use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "rift-http-proxy")]
struct Args {
    #[arg(short, long, default_value = "8080")]
    port: u16,
    #[arg(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    println!("Starting on port {}", args.port);
    tokio::signal::ctrl_c().await.ok();
}
