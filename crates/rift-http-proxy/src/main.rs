//! Rift HTTP Proxy - A Mountebank-compatible chaos engineering proxy
//!
//! Rift can run in two modes:
//! 1. **Mountebank mode** (default): Start with admin API on port 2525, create imposters dynamically
//! 2. **Rift native mode**: Load a Rift-specific YAML config file at startup
//!
//! # Examples
//!
//! Start in Mountebank mode (default):
//! ```bash
//! rift                                    # Admin API on port 2525
//! rift --port 3000                        # Admin API on port 3000
//! rift --configfile imposters.json        # Load imposters from file
//! rift --datadir ./mb-data                # Persist imposters to directory
//! ```
//!
//! Start in Rift native mode:
//! ```bash
//! rift --rift-config config.yaml          # Load Rift YAML config
//! ```

mod admin_api;
mod backends;
mod behaviors;
mod config;
mod fault;
mod flow_state;
mod imposter;
mod matcher;
mod metrics;
mod predicate;
mod proxy;
mod recording;
mod routing;
mod rule_index;
mod scripting;
mod template;

use admin_api::AdminApiServer;
use clap::{Parser, Subcommand};
use imposter::{ImposterConfig, ImposterManager};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Rift - A Mountebank-compatible HTTP chaos engineering proxy
///
/// By default, Rift starts in Mountebank mode with an admin API on port 2525.
/// Use --rift-config to start in Rift native mode with a YAML config file.
#[derive(Parser, Debug)]
#[command(name = "rift")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    // === Mountebank-compatible options ===
    /// Port for the admin API (Mountebank mode)
    #[arg(long, default_value = "2525", env = "MB_PORT")]
    port: u16,

    /// Hostname to bind the admin API to
    #[arg(long, default_value = "0.0.0.0", env = "MB_HOST")]
    host: String,

    /// Load imposters from a config file on startup (JSON or EJS format)
    #[arg(long, value_name = "FILE", env = "MB_CONFIGFILE")]
    configfile: Option<PathBuf>,

    /// Directory for persistent imposter storage
    #[arg(long, value_name = "DIR", env = "MB_DATADIR")]
    datadir: Option<PathBuf>,

    /// Allow JavaScript injection in responses (for inject and decorate)
    #[arg(long, env = "MB_ALLOW_INJECTION")]
    allow_injection: bool,

    /// Only accept requests from localhost
    #[arg(long, env = "MB_LOCAL_ONLY")]
    local_only: bool,

    /// Log level (debug, info, warn, error)
    #[arg(long, default_value = "info", env = "MB_LOGLEVEL")]
    loglevel: String,

    /// Don't write to log file (stdout only)
    #[arg(long)]
    nologfile: bool,

    /// Log file path (default: mb.log in current directory)
    #[arg(long, value_name = "FILE")]
    log: Option<PathBuf>,

    /// PID file path
    #[arg(long, value_name = "FILE")]
    pidfile: Option<PathBuf>,

    /// CORS allowed origin
    #[arg(long)]
    origin: Option<String>,

    /// IP addresses allowed to connect (comma-separated)
    #[arg(long, value_delimiter = ',')]
    ip_whitelist: Option<Vec<String>>,

    /// Run in mock mode (all imposters are mocks)
    #[arg(long)]
    mock: bool,

    /// Enable debug mode
    #[arg(long)]
    debug: bool,

    // === Rift-specific options ===
    /// Load Rift native YAML config file (alternative to Mountebank mode)
    #[arg(long, value_name = "FILE", env = "RIFT_CONFIG_PATH")]
    rift_config: Option<PathBuf>,

    /// Number of worker threads (Rift native mode only)
    #[arg(long, default_value = "0", env = "RIFT_WORKERS")]
    workers: usize,

    /// Metrics server port
    #[arg(long, default_value = "9090", env = "RIFT_METRICS_PORT")]
    metrics_port: u16,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the Rift server (default command)
    Start,

    /// Stop a running Rift server
    Stop {
        /// PID file to read for the process to stop
        #[arg(long, default_value = "rift.pid")]
        pidfile: PathBuf,
    },

    /// Restart the Rift server
    Restart {
        /// PID file to read for the process to restart
        #[arg(long, default_value = "rift.pid")]
        pidfile: PathBuf,
    },

    /// Save current imposters to a file
    Save {
        /// Output file path
        #[arg(long, required = true)]
        savefile: PathBuf,

        /// Include recorded requests in output
        #[arg(long)]
        remove_proxies: bool,
    },

    /// Replay saved imposters
    Replay {
        /// Input file path
        #[arg(long, required = true)]
        configfile: PathBuf,
    },
}

fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();

    // Install default cryptographic provider for rustls
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("Failed to install default crypto provider"))?;

    // Initialize tracing based on loglevel
    let log_level = match cli.loglevel.to_lowercase().as_str() {
        "debug" => "debug",
        "warn" | "warning" => "warn",
        "error" => "error",
        _ => "info",
    };

    let filter = if cli.debug { "debug" } else { log_level };

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)))
        .init();

    // Write PID file if requested
    if let Some(ref pidfile) = cli.pidfile {
        let pid = std::process::id();
        std::fs::write(pidfile, pid.to_string())?;
        info!("Wrote PID {} to {:?}", pid, pidfile);
    }

    // Handle subcommands
    match &cli.command {
        Some(Commands::Stop { pidfile }) => {
            return stop_server(pidfile);
        }
        Some(Commands::Restart { pidfile }) => {
            stop_server(pidfile)?;
            // Fall through to start
        }
        Some(Commands::Save { savefile, .. }) => {
            return save_imposters(&cli, savefile);
        }
        Some(Commands::Replay { configfile }) => {
            // Load the config file and start
            return run_mountebank_mode(Cli {
                configfile: Some(configfile.clone()),
                ..cli
            });
        }
        Some(Commands::Start) | None => {
            // Default behavior - check which mode to use
        }
    }

    // Determine which mode to run
    if let Some(ref config_path) = cli.rift_config {
        // Rift native mode
        info!(
            "Starting in Rift native mode with config: {:?}",
            config_path
        );
        run_rift_native_mode(&cli, config_path)
    } else {
        // Mountebank mode (default)
        info!("Starting in Mountebank mode on port {}", cli.port);
        run_mountebank_mode(cli)
    }
}

/// Run in Mountebank-compatible mode
fn run_mountebank_mode(cli: Cli) -> Result<(), anyhow::Error> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        // Create imposter manager
        let manager = Arc::new(ImposterManager::new());

        // Load imposters from configfile if provided
        if let Some(ref configfile) = cli.configfile {
            load_imposters_from_file(&manager, configfile).await?;
        }

        // Load imposters from datadir if provided
        if let Some(ref datadir) = cli.datadir {
            load_imposters_from_datadir(&manager, datadir).await?;
        }

        // Start metrics server
        let metrics_port = cli.metrics_port;
        tokio::spawn(async move {
            if let Err(e) = run_metrics_server(metrics_port).await {
                error!("Metrics server error: {}", e);
            }
        });

        // Determine bind address
        let host = if cli.local_only {
            "127.0.0.1"
        } else {
            &cli.host
        };

        let addr: SocketAddr = format!("{}:{}", host, cli.port).parse()?;

        // Start admin API server
        info!(
            "Rift Admin API (Mountebank-compatible) starting on http://{}",
            addr
        );
        info!(
            "Metrics available at http://{}:{}/metrics",
            host, metrics_port
        );

        if cli.allow_injection {
            info!("JavaScript injection enabled");
        }

        let server = AdminApiServer::new(addr, manager);
        server.run().await?;

        Ok(())
    })
}

/// Run in Rift native mode with YAML config
fn run_rift_native_mode(cli: &Cli, config_path: &PathBuf) -> Result<(), anyhow::Error> {
    use config::Config;
    use proxy::ProxyServer;

    info!("Loading Rift configuration from: {:?}", config_path);

    // Load initial configuration
    let config = Config::from_file(config_path.to_str().unwrap())?;

    // Start metrics server
    let metrics_port = config.metrics.port;
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            if let Err(e) = run_metrics_server(metrics_port).await {
                error!("Metrics server error: {}", e);
            }
        });
    });

    // Determine number of workers
    let num_workers = if cli.workers == 0 {
        if config.listen.workers == 0 {
            num_cpus::get()
        } else {
            config.listen.workers
        }
    } else {
        cli.workers
    };

    // Create shared flow store once if configured
    let shared_flow_store: Option<std::sync::Arc<dyn flow_state::FlowStore>> =
        if let Some(ref fs_config) = config.flow_state {
            info!(
                "Creating shared flow store ({} backend) for all workers",
                fs_config.backend
            );
            Some(flow_state::create_flow_store(fs_config)?)
        } else if !config.script_rules.is_empty() {
            info!("Using shared NoOpFlowStore for all workers (flow_state not configured)");
            Some(std::sync::Arc::new(flow_state::NoOpFlowStore))
        } else {
            None
        };

    info!("Starting {} worker threads", num_workers);

    // Spawn worker threads with shared flow store
    let handles: Vec<_> = (0..num_workers)
        .map(|worker_id| {
            let config_clone = config.clone();
            let flow_store_clone = shared_flow_store.clone();
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();

                runtime.block_on(async move {
                    info!("Worker {} starting", worker_id);
                    let server = if let Some(flow_store) = flow_store_clone {
                        ProxyServer::new_with_shared_flow_store(config_clone, flow_store).await?
                    } else {
                        ProxyServer::new(config_clone).await?
                    };
                    server.run().await
                })
            })
        })
        .collect();

    // Wait for all workers
    for (idx, handle) in handles.into_iter().enumerate() {
        match handle.join() {
            Ok(Ok(())) => info!("Worker {} exited normally", idx),
            Ok(Err(e)) => error!("Worker {} failed: {}", idx, e),
            Err(e) => error!("Worker {} panicked: {:?}", idx, e),
        }
    }

    Ok(())
}

/// Load imposters from a JSON config file
async fn load_imposters_from_file(
    manager: &Arc<ImposterManager>,
    path: &PathBuf,
) -> Result<(), anyhow::Error> {
    info!("Loading imposters from configfile: {:?}", path);

    let content = std::fs::read_to_string(path)?;

    // Try to parse as JSON (Mountebank format)
    let imposters: Vec<ImposterConfig> = if content.trim().starts_with('{') {
        // Single imposter or wrapper object
        let value: serde_json::Value = serde_json::from_str(&content)?;
        if let Some(imposters) = value.get("imposters") {
            serde_json::from_value(imposters.clone())?
        } else {
            // Single imposter
            vec![serde_json::from_value(value)?]
        }
    } else if content.trim().starts_with('[') {
        // Array of imposters
        serde_json::from_str(&content)?
    } else {
        // Try YAML
        serde_yaml::from_str(&content)?
    };

    for config in imposters {
        info!(
            "Creating imposter on port {:?} from configfile",
            config.port
        );
        match manager.create_imposter(config).await {
            Ok(port) => info!("Created imposter on port {}", port),
            Err(e) => error!("Failed to create imposter: {}", e),
        }
    }

    Ok(())
}

/// Load imposters from a data directory
async fn load_imposters_from_datadir(
    manager: &Arc<ImposterManager>,
    datadir: &PathBuf,
) -> Result<(), anyhow::Error> {
    info!("Loading imposters from datadir: {:?}", datadir);

    if !datadir.exists() {
        std::fs::create_dir_all(datadir)?;
        return Ok(());
    }

    for entry in std::fs::read_dir(datadir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map(|e| e == "json").unwrap_or(false) {
            let content = std::fs::read_to_string(&path)?;
            if let Ok(config) = serde_json::from_str::<ImposterConfig>(&content) {
                info!("Loading imposter on port {:?} from {:?}", config.port, path);
                match manager.create_imposter(config).await {
                    Ok(port) => info!("Created imposter on port {} from {:?}", port, path),
                    Err(e) => error!("Failed to create imposter from {:?}: {}", path, e),
                }
            }
        }
    }

    Ok(())
}

/// Stop a running server by PID file
fn stop_server(pidfile: &PathBuf) -> Result<(), anyhow::Error> {
    if !pidfile.exists() {
        return Err(anyhow::anyhow!("PID file not found: {pidfile:?}"));
    }

    let pid_str = std::fs::read_to_string(pidfile)?;
    let pid: i32 = pid_str.trim().parse()?;

    info!("Stopping server with PID {}", pid);

    #[cfg(unix)]
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }

    #[cfg(windows)]
    {
        // On Windows, use taskkill
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()?;
    }

    // Remove PID file
    std::fs::remove_file(pidfile)?;

    Ok(())
}

/// Save imposters to a file
fn save_imposters(cli: &Cli, savefile: &PathBuf) -> Result<(), anyhow::Error> {
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        let client = reqwest::Client::new();
        let url = format!("http://{}:{}/imposters?replayable=true", cli.host, cli.port);

        let response = client.get(&url).send().await?;
        let content = response.text().await?;

        std::fs::write(savefile, &content)?;
        info!("Saved imposters to {:?}", savefile);

        Ok(())
    })
}

/// Run the metrics server
async fn run_metrics_server(port: u16) -> anyhow::Result<()> {
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{body::Incoming, Request, Response};
    use hyper_util::rt::TokioIo;
    use std::convert::Infallible;
    use tokio::net::TcpListener;

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!("Metrics server listening on http://{}/metrics", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);

        tokio::spawn(async move {
            let service = service_fn(move |req: Request<Incoming>| async move {
                if req.uri().path() == "/metrics" {
                    let metrics = metrics::collect_metrics();
                    Ok::<_, Infallible>(Response::new(metrics))
                } else {
                    Ok::<_, Infallible>(
                        Response::builder()
                            .status(404)
                            .body("Not Found\n".to_string())
                            .unwrap(),
                    )
                }
            });

            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                error!("Metrics server connection error: {}", err);
            }
        });
    }
}
