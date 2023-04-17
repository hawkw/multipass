use crate::{
    discover::Name,
    route::{Recognize, RoutingTable},
    svc,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, path::Path, sync::Arc, time::Duration};

#[derive(Clone, Debug)]
pub struct Config {
    pub listeners: Listeners,
    pub admin: Option<SocketAddr>,
    pub local_tld: String,
    pub dyn_dns: Option<DynDns>,
    pub services: HashMap<Name, Domain>,
    pub routes: RoutingTable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Domain {
    #[serde(flatten)]
    pub recognize: Recognize,

    #[serde(default = "Domain::default_ty_domain")]
    pub service: String,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Listeners {
    #[serde(default = "Listeners::default_http")]
    pub http: SocketAddr,

    #[serde(default = "Listeners::default_https")]
    pub https: SocketAddr,

    #[serde(default)]
    pub queue: QueueConfig,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
struct Admin {
    addr: Option<SocketAddr>,
    enabled: bool,

    #[serde(default)]
    queue: QueueConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    listen: Listeners,

    #[serde(default)]
    admin: Admin,

    #[serde(default = "Config::default_local_tld")]
    local_tld: String,

    services: HashMap<String, Domain>,

    dyn_dns: Option<DynDns>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DynDns {
    Namecheap {
        token: String,
        domain: String,
        subdomains: Vec<String>,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueueConfig {
    #[serde(default = "QueueConfig::default_capacity")]
    capacity: usize,

    #[serde(default = "QueueConfig::default_timeout")]
    timeout: Duration,
}

// === impl Config ===

impl Config {
    fn default_local_tld() -> String {
        String::from("local")
    }

    pub fn load(path: &impl AsRef<Path>) -> anyhow::Result<Arc<Self>> {
        let path = path.as_ref();
        let file = std::fs::read_to_string(path)
            .with_context(|| format!("failed to open config file '{}'", path.display()))?;
        let ConfigFile {
            services,
            local_tld,
            dyn_dns,
            listen,
            admin,
        } = toml::from_str(&file)
            .with_context(|| format!("failed to parse config file '{}'", path.display()))?;

        let admin = if admin.enabled {
            admin.addr.or(Some(Admin::default_addr()))
        } else if let Some(addr) = admin.addr {
            anyhow::bail!("Admin server is disabled, but an address is provided: {addr}")
        } else {
            None
        };

        let services: HashMap<Name, Domain> = services
            .into_iter()
            .map(|(name, domain)| {
                let name = Name::from(format!("{name}.{local_tld}."));
                (name, domain)
            })
            .collect();
        let routes = services
            .iter()
            .map(|(name, domain)| (domain.recognize.clone(), name.clone()))
            .collect();

        Ok(Arc::new(Self {
            local_tld,
            services,
            dyn_dns,
            listeners: listen,
            admin,
            routes,
        }))
    }
}

// === impl Domain ===

impl Domain {
    fn default_ty_domain() -> String {
        String::from("_http._tcp")
    }
}

// === impl Listeners ===

impl Listeners {
    fn default_http() -> SocketAddr {
        SocketAddr::from(([0, 0, 0, 0], 80))
    }

    fn default_https() -> SocketAddr {
        SocketAddr::from(([0, 0, 0, 0], 443))
    }
}

impl Default for Listeners {
    fn default() -> Self {
        Self {
            http: Self::default_http(),
            https: Self::default_https(),
            queue: QueueConfig::default(),
        }
    }
}

// === impl Admin ===

impl Admin {
    fn default_addr() -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 6660))
    }
}

impl Default for Admin {
    fn default() -> Self {
        Self {
            addr: Some(Self::default_addr()),
            queue: Default::default(),
            enabled: true,
        }
    }
}

// === impl QueueConfig ===

impl QueueConfig {
    const fn default_capacity() -> usize {
        1000
    }

    const fn default_timeout() -> Duration {
        Duration::from_secs(5)
    }
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            capacity: Self::default_capacity(),
            timeout: Self::default_timeout(),
        }
    }
}

impl<T> svc::ExtractParam<svc::queue::Capacity, T> for QueueConfig {
    fn extract_param(&self, _: &T) -> svc::queue::Capacity {
        svc::queue::Capacity(self.capacity)
    }
}

impl<T> svc::ExtractParam<svc::queue::Timeout, T> for QueueConfig {
    fn extract_param(&self, _: &T) -> svc::queue::Timeout {
        svc::queue::Timeout(self.timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route() {
        let toml = r#"
[services."eclss"]
service = "_https._tcp.local."
path_regex = "/eclss/*"
"#;
        let ConfigFile { services, .. } = dbg!(toml::from_str(toml)).unwrap();
        let eclss = services
            .get("eclss")
            .expect("config file must have 'eclss' service");
        assert_eq!(eclss.service, "_https._tcp.local.");
        assert_eq!(
            eclss.recognize.path_regex.as_ref().map(|r| r.as_str()),
            Some("/eclss/*")
        );
    }

    #[test]
    fn listeners() {
        let toml = r#"
        [services."eclss"]
        service = "_https._tcp.local."
        "#;
        let ConfigFile { listen, .. } = dbg!(toml::from_str(toml)).unwrap();
        assert_eq!(dbg!(listen), Listeners::default());

        let toml = r#"
        [listen]
        http = "0.0.0.0:8080"

        [services."eclss"]
        service = "_https._tcp.local."
        "#;
        let ConfigFile { listen, .. } = dbg!(toml::from_str(toml)).unwrap();
        assert_eq!(
            dbg!(listen),
            Listeners {
                http: SocketAddr::from(([0, 0, 0, 0], 8080)),
                ..Listeners::default()
            }
        );
    }
}
