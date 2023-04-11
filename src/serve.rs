use anyhow::Context;
use futures::{Stream, StreamExt};
use linkerd_stack as svc;
use std::{future::Future, net::SocketAddr};
use tokio::{
    io,
    net::{TcpListener, TcpStream},
};
use tokio_stream::wrappers::TcpListenerStream;
use tracing::Instrument;

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

pub async fn serve<I, F>(
    listen: impl Stream<Item = io::Result<(I, SocketAddr)>>,
    shutdown: impl Future + Send,
    mut accept: impl FnMut(I) -> F,
) where
    I: io::AsyncRead + io::AsyncWrite + Send + 'static,
    F: Future<Output = io::Result<()>> + Send + 'static,
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
            tokio::spawn(accept(conn).instrument(span.exit().or_current()));
        }
    };

    tokio::select! {
        _ = accept => {}
        _ = shutdown => { tracing::info!("shutdown signal received, shutting down...")}
    }
}
