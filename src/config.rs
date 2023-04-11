use crate::route::Recognize;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, path::Path};

#[derive(Clone, Debug)]
pub struct Config {
    pub addrs: Addrs,
    pub local_tld: String,
    pub dyn_dns: Option<DynDns>,
    pub services: HashMap<String, Domain>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Domain {
    #[serde(flatten)]
    pub recognize: Recognize,
    #[serde(default = "Domain::default_ty_domain")]
    pub service: String,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Addrs {
    #[serde(default = "Addrs::default_http_listener")]
    pub http: SocketAddr,
    #[serde(default = "Addrs::default_https_listener")]
    pub https: SocketAddr,
    #[serde(default = "Addrs::default_admin_listener")]
    pub admin: SocketAddr,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    addrs: Addrs,
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

impl Addrs {
    fn default_http_listener() -> SocketAddr {
        SocketAddr::from(([0, 0, 0, 0], 80))
    }

    fn default_https_listener() -> SocketAddr {
        SocketAddr::from(([0, 0, 0, 0], 443))
    }

    fn default_admin_listener() -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 6660))
    }
}

impl Default for Addrs {
    fn default() -> Self {
        Self {
            http: Self::default_http_listener(),
            https: Self::default_https_listener(),
            admin: Self::default_admin_listener(),
        }
    }
}

// === impl Config ===

impl Config {
    fn default_local_tld() -> String {
        String::from("local")
    }

    pub fn load(path: &impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let file = std::fs::read_to_string(path)
            .with_context(|| format!("failed to open config file '{}'", path.display()))?;
        let ConfigFile {
            services,
            local_tld,
            dyn_dns,
            addrs,
        } = toml::from_str(&file)
            .with_context(|| format!("failed to parse config file '{}'", path.display()))?;
        Ok(Self {
            local_tld,
            services,
            dyn_dns,
            addrs,
        })
    }
}

// === impl Domain ===

impl Domain {
    fn default_ty_domain() -> String {
        String::from("._http._tcp")
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
    fn addrs() {
        let toml = r#"
        [services."eclss"]
        service = "_https._tcp.local."
        "#;
        let ConfigFile { addrs, .. } = dbg!(toml::from_str(toml)).unwrap();
        assert_eq!(dbg!(addrs), Addrs::default());

        let toml = r#"
        [addrs]
        http = "0.0.0.0:8080"

        [services."eclss"]
        service = "_https._tcp.local."
        "#;
        let ConfigFile { addrs, .. } = dbg!(toml::from_str(toml)).unwrap();
        assert_eq!(
            dbg!(addrs),
            Addrs {
                http: SocketAddr::from(([0, 0, 0, 0], 8080)),
                ..Addrs::default()
            }
        );
    }
}
