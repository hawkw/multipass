use anyhow::Context;
use clap::Parser;
use std::path::PathBuf;

use multipass::{config::Config, discover::MdnsDiscover, serve, svc, Proxy};
use std::net::SocketAddr;
use tokio::net::TcpStream;
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
            .with_thread_ids(true)
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
    let connect = svc::service_fn(|addr: SocketAddr| Box::pin(TcpStream::connect(addr)));

    let http_server = {
        let sock = serve::bind(listeners.http)
            .await
            .context("failed to bind HTTP listener")?;
        let http = Proxy::new(config.clone(), connect)
            .push_http_endpoint()
            .push_http_discover(&discover)
            .push_http_server()
            .into_inner();
        let serve = serve::serve(listeners.http, sock, tokio::signal::ctrl_c(), http)
            .instrument(tracing::info_span!("serve_http", addr = %listeners.http));
        tokio::spawn(serve)
    };
    http_server.await?;
    Ok(())
}
