use crate::svc;
use hyper::body::Incoming;
pub use hyper_util::client::*;
use hyper_util::rt::TokioExecutor;
pub use legacy::Client;
use std::{
    net::SocketAddr,
    task::{Context, Poll},
};
use tokio::io;

#[derive(Clone, Debug)]
pub struct NewClient<C> {
    connect: C,
}

#[derive(Clone, Debug)]
pub struct Connect<C> {
    addr: SocketAddr,
    connect: C,
}

impl<C> NewClient<C> {
    pub fn layer() -> impl svc::Layer<C, Service = Self> + Clone {
        svc::layer::mk(|connect| Self { connect })
    }
}

impl<C, T, I> svc::NewService<T> for NewClient<C>
where
    C: svc::Service<SocketAddr, Response = I> + Clone + Send + 'static,
    C::Future: Send + Unpin,
    C::Error: std::error::Error + Send + Sync,
    I: io::AsyncRead + io::AsyncWrite + connect::Connection + Unpin + Send + 'static,
    T: svc::Param<SocketAddr>,
{
    type Service = Client<Connect<C>, Incoming>;

    fn new_service(&self, target: T) -> Self::Service {
        let addr = target.param();
        let connect = Connect {
            addr,
            connect: self.connect.clone(),
        };
        Client::builder(TokioExecutor::new()).build(connect)
    }
}

// === impl Connect ===

impl<C> svc::Service<hyper::Uri> for Connect<C>
where
    C: svc::Service<SocketAddr>,
{
    type Error = C::Error;
    type Response = C::Response;
    type Future = C::Future;
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.connect.poll_ready(cx)
    }

    fn call(&mut self, _: hyper::Uri) -> Self::Future {
        self.connect.call(self.addr)
    }
}
