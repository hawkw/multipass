use anyhow::Context;
use clap::Parser;
use std::path::PathBuf;

use multipass::{config::Config, discover::MdnsDiscover, serve};
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

    let http_server = {
        let http = serve::bind(listeners.http)
            .await
            .context("failed to bind HTTP listener")?;
        let serve = serve::serve(http, tokio::signal::ctrl_c(), |_conn| async move {
            todo!("eliza: serve HTTP")
        })
        .instrument(tracing::info_span!("serve_http", addr = %listeners.http));
        tokio::spawn(serve)
    };

    let test = tokio::spawn({
        let discover = discover.clone();
        async move {
            let mut test = discover
                .clone()
                .oneshot("noctis.local.".into())
                .await
                .unwrap();
            loop {
                tracing::info!(services = ?test.borrow_and_update());
                tokio::select! {
                    res = test.changed() => { if res.is_err() { return; } },
                    _ = tokio::signal::ctrl_c() => { return; },
                }
            }
        }
    });

    tokio::try_join! {
        http_server,
        test,
    }?;

    Ok(())
}
