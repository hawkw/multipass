use anyhow::Context;
use futures::{Stream, StreamExt};
use hyper::{
    body::{Body, Incoming},
    service::service_fn,
    Request, Response,
};
use hyper_util::{rt::tokio_executor::TokioExecutor, server::conn::auto};
use linkerd_stack as svc;
use std::{future::Future, net::SocketAddr};
use tokio::{
    io,
    net::{TcpListener, TcpStream},
};
use tokio_stream::wrappers::TcpListenerStream;
use tracing::Instrument;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Accepted {
    client_addr: SocketAddr,
}

pub async fn bind(
    addr: SocketAddr,
) -> anyhow::Result<impl Stream<Item = io::Result<(TcpStream, SocketAddr)>>> {
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    Ok(TcpListenerStream::new(listener).map(|res| {
        let sock = res?;
        let addr = sock.peer_addr()?;
        Ok((sock, addr))
    }))
}

pub async fn serve<I, S, B>(
    listen: impl Stream<Item = io::Result<(I, SocketAddr)>>,
    shutdown: impl Future + Send,
    new_svc: impl svc::NewService<Accepted, Service = S> + Clone + Send + 'static,
) where
    I: io::AsyncRead + io::AsyncWrite + Send + Unpin + 'static,
    S: svc::Service<Request<Incoming>, Response = Response<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    B: Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let accept = async move {
        tokio::pin! {
            let listen = listen;
        }
        loop {
            let (conn, addr) = match listen.next().await {
                Some(Ok(conn)) => conn,
                Some(Err(error)) => {
                    tracing::error!(%error, "failed to accept connection!");
                    continue;
                }
                None => {
                    tracing::info!("listener stream closed, shutting down...");
                    break;
                }
            };

            let span = tracing::debug_span!("conn", client.addr = %addr).entered();
            tracing::debug!("accepted connection");
            let svc = NewHyperService {
                new_svc: new_svc.clone(),
                client_addr: addr,
            };
            tokio::spawn(
                async move {
                    auto::Builder::new(TokioExecutor::new())
                        .http1()
                        .keep_alive(true)
                        .http2()
                        .keep_alive_interval(None)
                        .serve_connection(conn, svc)
                        .await
                }
                .instrument(span.exit().or_current()),
            );
        }
    };

    tokio::select! {
        _ = accept => {}
        _ = shutdown => { tracing::info!("shutdown signal received, shutting down...")}
    }
}

#[derive(Clone, Debug)]
struct NewHyperService<N> {
    new_svc: N,
    client_addr: SocketAddr,
}

impl<N, S, B, R> hyper::service::Service<R> for NewHyperService<N>
where
    N: svc::NewService<Accepted, Service = S>,
    S: svc::Service<R, Response = Response<B>> + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    B: Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Error = S::Error;
    type Future = svc::Oneshot<S, R>;
    type Response = S::Response;

    fn call(&mut self, req: R) -> Self::Future {
        use svc::ServiceExt;
        self.new_svc
            .new_service(Accepted {
                client_addr: self.client_addr,
            })
            .oneshot(req)
    }
}
