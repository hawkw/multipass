use crate::config::{self, Config};
use ahash::AHashMap;
use anyhow::Context;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::{sync::Arc, task::Poll};
use tokio::sync::watch;
use tracing::Instrument;

#[derive(Clone)]
pub struct MdnsDiscover {
    domains: Arc<AHashMap<Arc<str>, Receiver>>,
    _daemon: ServiceDaemon,
}

pub type Receiver = watch::Receiver<Option<ServiceInfo>>;

impl MdnsDiscover {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let daemon = ServiceDaemon::new()?;
        let mut ty_domains: AHashMap<&str, AHashMap<Arc<str>, _>> = AHashMap::new();
        let domains = config
            .services
            .iter()
            .map(|(name, config::Domain { ref service, .. })| {
                let (tx, rx) = tokio::sync::watch::channel(None);
                let name: Arc<str> = Arc::from(format!("{name}.{}.", config.local_tld));
                ty_domains
                    .entry(service)
                    .or_default()
                    .insert(name.clone(), tx);
                (name, rx)
            })
            .collect();

        for (service_type, mut watches) in ty_domains {
            let browse = daemon
                .browse(service_type)
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
                                        tracing::info!(?service, "Service '{name}' resolved");
                                        tx.send_replace(Some(service));
                                    }
                                    None => tracing::debug!(
                                        ?service,
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

impl tower::Service<String> for MdnsDiscover {
    type Response = Receiver;
    type Error = anyhow::Error;
    type Future = futures::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, target: String) -> Self::Future {
        futures::future::ready(
            self.domains
                .get(target.as_str())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No service configured for {target}")),
        )
    }
}
