use crate::{discover, route::RoutingTable, serve, svc, Proxy};
mod client;
mod header_from_target;
pub use self::client::{Connect, NewClient};
pub use http::*;

use hyper::body::Incoming;
use std::{net::SocketAddr, ops::Deref};
use tokio::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route<T> {
    parent: T,
    name: discover::Name,
}

impl<C> Proxy<C>
where
    C: svc::Service<SocketAddr> + Clone + Send + Sync + 'static,
    C::Future: Send + Unpin,
    C::Error: std::error::Error + Send + Sync,
    C::Response: io::AsyncRead + io::AsyncWrite + client::connect::Connection,
    C::Response: Unpin + Send + 'static,
{
    pub fn push_http_endpoint(
        self,
    ) -> Proxy<
        impl svc::NewService<
                discover::Discovered,
                Service = impl svc::Service<
                    Request<Incoming>,
                    Response = Response<Incoming>,
                    Error = linkerd_app_core::Error,
                    Future = impl std::future::Future + Send,
                > + Clone
                              + Send
                              + Sync
                              + 'static,
            > + Clone
            + Send,
    > {
        self.map_stack(|connect, _| {
            connect
                .push(NewClient::layer())
                // .push_on_service(svc::util::MapResponseLayer::new(
                //     |rsp: http::Response<Incoming>| rsp.map(http_body_util::Either::Right),
                // ))
                // Convert origin form HTTP/1 URIs to absolute form for Hyper's
                // `Client`.
                .push(linkerd_app_core::proxy::http::NewNormalizeUri::layer())
                .instrument(
                    |d: &discover::Discovered| tracing::info_span!("endpoint", addr = %d.addr),
                )
        })
    }
}

impl<N, S> Proxy<N>
where
    N: svc::NewService<discover::Discovered, Service = S>,
    N: Clone + Send + Sync + 'static,
    S: svc::Service<
        Request<Incoming>,
        Response = http::Response<Incoming>,
        Error = linkerd_app_core::Error,
    >,
    S: Clone + Send + Sync + 'static,
    S::Future: Send,
{
    pub fn push_http_discover<T>(
        self,
        discover: &discover::MdnsDiscover,
    ) -> Proxy<
        svc::ArcNewService<
            T,
            impl svc::Service<
                    Request<Incoming>,
                    Response = Response<Incoming>,
                    Error = linkerd_app_core::Error,
                    Future = impl std::future::Future + Send,
                > + Clone
                + Send
                + Sync,
        >,
    >
    where
        T: svc::Param<discover::Name> + Clone + Send + Sync + 'static,
    {
        let discover = svc::stack(discover.clone())
            .push(svc::MapErr::layer_boxed())
            .into_inner();
        self.map_stack(move |endpoint, cfg| {
            endpoint
                .push(svc::FilterLayer::new(
                    |discovered: Option<discover::Discovered>| {
                        discovered.ok_or_else(discover::NotResolved::default)
                    },
                ))
                .check_new_clone::<Option<discover::Discovered>>()
                .lift_new()
                .push(svc::NewSpawnWatch::<Option<discover::Discovered>, _>::layer())
                .check_new_service::<discover::Receiver, _>()
                .lift_new()
                .push_new_cached_discover::<discover::Name, _>(
                    discover,
                    std::time::Duration::from_secs(60),
                )
                .push(svc::NewQueue::layer_via(cfg.listeners.queue))
                .instrument(|t: &T| {
                    let name = t.param();
                    tracing::info_span!("route", %name)
                })
                .push(svc::ArcNewService::layer())
        })
    }
}

impl<N> Proxy<N> {
    pub fn push_http_server<S>(
        self,
    ) -> Proxy<
        impl svc::NewService<
                serve::Accepted,
                Service = impl svc::Service<
                    http::Request<Incoming>,
                    Response = http::Response<Incoming>,
                    Error = linkerd_app_core::Error,
                    Future = impl std::future::Future + Send,
                > + Clone
                              + Send,
            > + Clone
            + Send,
    >
    where
        N: svc::NewService<Route<serve::Accepted>, Service = S> + Clone + Send,
        S: svc::Service<
            Request<Incoming>,
            Response = http::Response<Incoming>,
            Error = linkerd_app_core::Error,
        >,
        S: Clone + Send,
        S::Future: Send,
        S::Response: Send,
    {
        self.map_stack(|discover, cfg| {
            discover
                .push(header_from_target::NewHeaderFromTarget::layer_via(
                    |route: &Route<serve::Accepted>| {
                        let client_addr = route.parent.client_addr;

                        (
                            http::header::FORWARDED,
                            format!("for={client_addr};host={}", route.name)
                                .parse::<http::HeaderValue>()
                                .unwrap(),
                        )
                    },
                ))
                .lift_new_with_target()
                .check_new_new::<serve::Accepted, discover::Name>()
                .push(
                    linkerd_router::NewOneshotRoute::<RoutingTable, _, _>::layer_via({
                        let routes = cfg.routes.clone();
                        move |_: &serve::Accepted| routes.clone()
                    }),
                )
                .instrument(|_: &serve::Accepted| tracing::info_span!("http"))
                .check_clone()
                .check_new_service::<serve::Accepted, _>()
        })
    }
}

// === impl Route ===

impl<T> svc::Param<discover::Name> for Route<T> {
    fn param(&self) -> discover::Name {
        self.name.clone()
    }
}

impl<T> Deref for Route<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.parent
    }
}

impl<T> From<(discover::Name, T)> for Route<T> {
    fn from((name, parent): (discover::Name, T)) -> Self {
        Self { name, parent }
    }
}
