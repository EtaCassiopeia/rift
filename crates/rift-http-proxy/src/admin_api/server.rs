//! Admin API server.

use crate::admin_api::router::route_request;
use crate::imposter::ImposterManager;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{debug, info};

/// Admin API server for Rift
pub struct AdminApiServer {
    addr: SocketAddr,
    manager: Arc<ImposterManager>,
}

impl AdminApiServer {
    /// Create a new admin API server
    pub fn new(addr: SocketAddr, manager: Arc<ImposterManager>) -> Self {
        Self { addr, manager }
    }

    /// Run the admin API server
    pub async fn run(self) -> Result<(), anyhow::Error> {
        let listener = TcpListener::bind(self.addr).await?;
        info!(
            "Rift Admin API (Mountebank-compatible) listening on http://{}",
            self.addr
        );

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let manager = Arc::clone(&self.manager);

            tokio::spawn(async move {
                let service = service_fn(move |req| {
                    let manager = Arc::clone(&manager);
                    async move { route_request(req, manager).await }
                });

                if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                    debug!("Admin API connection error: {}", e);
                }
            });
        }
    }
}
