use crate::route::Recognize;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

#[derive(Clone, Debug)]
pub struct Config {
    pub local_tld: String,
    pub dyn_dns: Option<DynDns>,
    pub services: HashMap<String, Domain>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Domain {
    #[serde(flatten)]
    pub recognize: Recognize,
    pub service: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ConfigFile {
    local_tld: Option<String>,
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

impl Config {
    pub fn load(path: &impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let file = std::fs::read_to_string(path)
            .with_context(|| format!("failed to open config file '{}'", path.display()))?;
        let ConfigFile {
            services,
            local_tld,
            dyn_dns,
        } = toml::from_str(&file)
            .with_context(|| format!("failed to parse config file '{}'", path.display()))?;
        Ok(Self {
            local_tld: local_tld.unwrap_or_else(|| String::from("local")),
            services,
            dyn_dns,
        })
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
        assert_eq!(eclss.service.as_deref(), Some("_https._tcp.local."));
        assert_eq!(
            eclss.recognize.path_regex.as_ref().map(|r| r.as_str()),
            Some("/eclss/*")
        );
    }
}
