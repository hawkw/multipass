use anyhow::Context;
use clap::Parser;
use std::path::PathBuf;

use multipass::{
    config::Config,
    discover::{self, MdnsDiscover},
    http_client,
    route::RoutingTable,
    serve, svc,
};
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tower::ServiceExt;
use tracing::Instrument;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Path to `multipass.toml`.
    #[arg(short, long, default_value = "/etc/multipass/multipass.toml")]
    config: PathBuf,

    #[arg(
        short,
        long,
        env = "MULTIPASS_LOG",
        default_value = "multipass=debug,warn"
    )]
    log: String,
}

impl Args {
    fn trace_init(&self) -> anyhow::Result<()> {
        let filter = self.log.parse::<tracing_subscriber::EnvFilter>()?;
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            // .pretty()
            .init();

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    args.trace_init().context("failed to initialize tracing!")?;

    tracing::info!("leeloo dallas mul-ti-pass!");
    tracing::debug!(args = format_args!("{args:#?}"));

    let config = Config::load(&args.config)?;
    tracing::debug!(config = format_args!("{config:#?}"));
    let listeners = config.listeners;
    tracing::info!(
        listeners.http = %listeners.http,
        listeners.https = %listeners.https,
        listeners.admin = ?config.admin,
        "Listening...",
    );

    let discover = MdnsDiscover::new(&config).context("failed to start discovery")?;
    let routing_table = RoutingTable::new(&config);
    let connect = svc::service_fn(|addr: SocketAddr| Box::pin(TcpStream::connect(addr)));

    let http_server = {
        let http = serve::bind(listeners.http)
            .await
            .context("failed to bind HTTP listener")?;
        let fail = svc::stack(svc::ArcNewService::new(|()| {
            svc::mk(move |_: http::Request<hyper::body::Incoming>| {
                futures::future::ready(
                    http::Response::builder()
                        .status(http::StatusCode::BAD_GATEWAY)
                        .body(http_body_util::Either::Left(http_body_util::Full::new(
                            bytes::Bytes::from("no mDNS service resolved for domain"),
                        ))),
                )
            })
        }))
        .check_new_service::<(), http::Request<hyper::body::Incoming>>()
        .into_inner();
        let stack = svc::stack(connect)
            .check_service::<SocketAddr>()
            .check_clone()
            .push(http_client::NewClient::layer())
            .push_on_service(svc::util::MapResponseLayer::new(|rsp: http::Response<hyper::body::Incoming>| rsp.map(http_body_util::Either::Right)))
            // Convert origin form HTTP/1 URIs to absolute form for Hyper's
            // `Client`.
            .push(linkerd_app_core::proxy::http::NewNormalizeUri::layer())
            .check_new_clone::<discover::Discovered>()
            .push_switch(
                |maybe_discovered: Option<discover::Discovered>| -> Result<_, linkerd_app_core::Infallible> {
                    Ok(match maybe_discovered {
                        Some(discovered) => svc::Either::A(discovered),
                        None => svc::Either::B(()),
                    })
                },
            fail)
            // .push(svc::Filter::layer(|d: Option<discover::Discovered>| -> Result<discover::Discovered, linkerd_app_core::Error> {
            //     d.ok_or_else(|| anyhow::anyhow!("no service discovered for this domain")).map_err(Into::into)
            // } as fn(_) -> _))
            .check_new_service::<Option<discover::Discovered>, _>()
            .lift_new()
            .push(svc::NewSpawnWatch::layer())
            .check_new_service::<discover::Receiver, _>()
            .lift_new()
            .push_new_cached_discover::<discover::Name, _>(discover.clone(), std::time::Duration::from_secs(60))
            .check_new_service::<discover::Name, _>()
            .lift_new()
            .push(linkerd_router::NewOneshotRoute::<RoutingTable, _, _>::layer_via({
                let routes = routing_table.clone();
                move |_: &serve::Accepted| routes.clone()
            }))
            .check_clone()
            .check_new_service::<serve::Accepted, _>();
        let serve = serve::serve(http, tokio::signal::ctrl_c(), stack)
            .instrument(tracing::info_span!("serve_http", addr = %listeners.http));
        tokio::spawn(serve)
    };

    // let test = tokio::spawn({
    //     let discover = discover.clone();
    //     async move {
    //         let mut test = discover
    //             .clone()
    //             .oneshot("noctis.local.".into())
    //             .await
    //             .unwrap();
    //         loop {
    //             tracing::info!(services = ?test.borrow_and_update());
    //             tokio::select! {
    //                 res = test.changed() => { if res.is_err() { return; } },
    //                 _ = tokio::signal::ctrl_c() => { return; },
    //             }
    //         }
    //     }
    // });

    // tokio::try_join! {
    //     http_server,
    //     test,
    // }?;
    http_server.await?;
    Ok(())
}
