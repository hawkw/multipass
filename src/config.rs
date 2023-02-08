use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub local_tld: String,
    pub services: HashMap<String, Service>,
    pub dyn_dns: Option<DynDns>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct ConfigFile {
    local_tld: Option<String>,
    services: HashMap<String, Service>,
    dyn_dns: Option<DynDns>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Service {
    pub service: Option<String>,
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
