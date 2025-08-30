use clap::Parser;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(name = "rift-http-proxy")]
struct Args {
    #[arg(short, long, default_value = "8080")]
    port: u16,
    #[arg(short, long)]
    config: Option<String>,
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let level = if args.verbose { Level::DEBUG } else { Level::INFO };
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    info!("Starting Rift on port {}", args.port);

    if let Some(config_path) = &args.config {
        match rift_http_proxy::config::Config::load(config_path) {
            Ok(config) => info!("Loaded config: {:?}", config),
            Err(e) => tracing::error!("Failed to load config: {}", e),
        }
    }

    tokio::signal::ctrl_c().await.ok();
    info!("Shutting down");
}
