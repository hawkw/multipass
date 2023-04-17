use crate::config::{self, Config};
use ahash::AHashMap;
use anyhow::Context;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    task::Poll,
};
use tokio::sync::watch;
use tracing::Instrument;

pub type Name = Arc<str>;

#[derive(Clone)]
pub struct MdnsDiscover {
    domains: Arc<AHashMap<Name, Receiver>>,
    _daemon: ServiceDaemon,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct Discovered {
    pub addr: SocketAddr,
    pub name: http::uri::Authority,
}

#[derive(Debug, Clone, thiserror::Error, Default)]
#[error("no mDNS service discovered for this domain")]
pub struct NotResolved(());

#[derive(Debug, Clone, thiserror::Error)]
#[error("mDNS service '{0}' not in configured domains")]
pub struct NotConfigured(Name);

pub type Receiver = watch::Receiver<Option<Discovered>>;

impl MdnsDiscover {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let daemon = ServiceDaemon::new()?;
        let mut ty_domains: AHashMap<&str, AHashMap<Name, _>> = AHashMap::new();
        let domains = config
            .services
            .iter()
            .map(|(name, config::Domain { ref service, .. })| {
                let (tx, rx) = tokio::sync::watch::channel(None);
                ty_domains
                    .entry(service)
                    .or_default()
                    .insert(name.clone(), tx);
                (name.clone(), rx)
            })
            .collect();

        for (service_type, mut watches) in ty_domains {
            let service_type = format!("{service_type}.{}.", config.local_tld);
            let browse = daemon
                .browse(&service_type)
                .with_context(|| format!("Failed to browse for {service_type}"))?;
            tokio::spawn(
                async move {
                    tracing::info!("Starting to browse...");
                    loop {
                        let event = browse.recv_async().await;
                        tracing::trace!(?event);
                        match event {
                            Err(error) => tracing::error!(%error),
                            Ok(ServiceEvent::ServiceResolved(service)) => {
                                let name = service.get_hostname();
                                match watches.get_mut(name) {
                                    Some(tx) => {
                                        tracing::info!(service = ?format_args!("{service:#?}"), "Service '{name}' resolved");
                                        let svc = Discovered::from_service_info(&service, name);
                                        tx.send_replace(svc);
                                    }
                                    None => tracing::debug!(
                                        service = ?format_args!("{service:#?}"),
                                        "Service {name} not in config, ignoring update"
                                    ),
                                }
                            }
                            Ok(ServiceEvent::ServiceRemoved(kind, name)) => {
                                match watches.get_mut(name.as_str()) {
                                    Some(tx) => {
                                        tracing::info!(kind, "Service '{name}' removed");
                                        tx.send_replace(None);
                                    }
                                    None => tracing::debug!(
                                        kind,
                                        "Service {name} not in config, ignoring removal"
                                    ),
                                }
                            }
                            Ok(_) => {}
                        }
                    }
                }
                .instrument(tracing::info_span!("browse", message = %service_type)),
            );
        }

        Ok(Self {
            domains: Arc::new(domains),
            _daemon: daemon,
        })
    }
}

impl tower::Service<Name> for MdnsDiscover {
    type Response = Receiver;
    type Error = NotConfigured;
    type Future = futures::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: Name) -> Self::Future {
        futures::future::ready(self.domains.get(&name).cloned().ok_or(NotConfigured(name)))
    }
}

// === impl Discovered ===

impl Discovered {
    fn from_service_info(info: &ServiceInfo, name: &str) -> Option<Self> {
        // TODO(eliza): construct a load balancer over all addresses?
        let addr = {
            let ip = info.get_addresses().iter().next()?;
            let port = info.get_port();
            SocketAddr::new(IpAddr::V4(*ip), port)
        };
        Some(Self {
            addr,
            name: name.parse().unwrap(),
        })
    }
}

impl crate::svc::Param<SocketAddr> for Discovered {
    fn param(&self) -> SocketAddr {
        self.addr
    }
}

impl crate::svc::Param<linkerd_app_core::proxy::http::normalize_uri::DefaultAuthority>
    for Discovered
{
    fn param(&self) -> linkerd_app_core::proxy::http::normalize_uri::DefaultAuthority {
        linkerd_app_core::proxy::http::normalize_uri::DefaultAuthority(Some(self.name.clone()))
    }
}
