use crate::config::{self, Config};
use anyhow::Context;
use linkerd_stack as stack;
// use simple_mdns::{async_discovery::ServiceDiscovery, InstanceInformation};
use ahash::AHashMap;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::{sync::Arc, task::Poll};
use tokio::sync::watch;
use tracing::Instrument;

#[derive(Clone)]
pub struct MdnsDiscover {
    config: Arc<Config>,
    ty_domains: AHashMap<String, Watch>,
    _daemon: ServiceDaemon,
}

impl MdnsDiscover {
    pub fn new(config: Arc<Config>) -> anyhow::Result<Self> {
        let daemon = ServiceDaemon::new()?;
        let ty_domains = config
            .services
            .values()
            .map(|config::Service { service }| service.as_deref().unwrap_or("_.http._tcp"))
            .collect::<ahash::AHashSet<_>>();
        let ty_domains = ty_domains
            .into_iter()
            .map(|service_kind| {
                let service_kind = service_kind.to_owned();
                let (tx, rx) = watch::channel(AHashMap::<String, _>::new());
                let browse = daemon
                    .browse(&service_kind)
                    .with_context(|| format!("Failed to browse for {}", service_kind))?;
                tracing::info!(service_kind, "starting to browse");
                tokio::spawn(
                    async move {
                        loop {
                            let event = browse.recv_async().await;
                            tracing::trace!(?event);
                            match event {
                                Err(error) => tracing::error!(%error),
                                Ok(ServiceEvent::ServiceResolved(service)) => {
                                    tracing::info!(?service, "service added");
                                    tx.send_modify(|services| {
                                        services.insert(service.get_hostname().to_owned(), service);
                                    });
                                }
                                Ok(ServiceEvent::ServiceRemoved(kind, service)) => {
                                    tracing::info!(service, kind, "service removed");
                                    tx.send_modify(|services| {
                                        services.remove(&service);
                                    });
                                }
                                Ok(_) => {}
                            }
                        }
                    }
                    .instrument(tracing::info_span!("browse", service_kind)),
                );

                Ok((service_kind, rx))
            })
            .collect::<anyhow::Result<AHashMap<String, _>>>()?;

        Ok(Self {
            config,
            ty_domains,
            _daemon: daemon,
        })
    }
}

pub type Watch = watch::Receiver<AHashMap<String, ServiceInfo>>;

impl tower::Service<String> for MdnsDiscover {
    type Response = Watch;
    type Error = anyhow::Error;
    type Future = futures::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, target: String) -> Self::Future {
        let service_kind: &str = self
            .config
            .services
            .get(&target)
            .and_then(|target| target.service.as_deref())
            .unwrap_or("_http._tcp.local");

        if let Some(watch) = self.ty_domains.get(service_kind) {
            // TODO(eliza): if the sender has been dropped, restart that
            // discovery task...
            tracing::info!(?target, ?service_kind, "using cached discovery");
            futures::future::ok(watch.clone())
        } else {
            futures::future::err(anyhow::anyhow!(
                "cannot discover '{target}': no discovery for service type '{service_kind}'",
            ))
        }
    }
}
