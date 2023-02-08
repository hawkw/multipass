use anyhow::Context;
use clap::Parser;
use std::{path::PathBuf, sync::Arc};

use multipass::config::Config;
use tower::ServiceExt;

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

    let discover = multipass::discover::MdnsDiscover::new(Arc::new(config))?;
    let mut test = discover.clone().oneshot("exocortex".into()).await?;
    loop {
        tracing::info!(services = ?test.borrow_and_update());
        test.changed().await?;
    }

    Ok(())
}
