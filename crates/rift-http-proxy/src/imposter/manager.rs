//! ImposterManager - lifecycle management for multiple imposters.
//!
//! This module handles creating, deleting, and managing multiple imposters,
//! each running on its own port.

use super::core::Imposter;
use super::handler::handle_imposter_request;
use super::types::{ImposterConfig, ImposterError, Stub};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

/// Manages the lifecycle of multiple imposters
pub struct ImposterManager {
    /// Active imposters by port
    imposters: RwLock<HashMap<u16, Arc<Imposter>>>,
    /// Global shutdown signal (for future graceful shutdown)
    #[allow(dead_code)]
    shutdown_tx: broadcast::Sender<()>,
}

impl ImposterManager {
    /// Create a new imposter manager
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);
        Self {
            imposters: RwLock::new(HashMap::new()),
            shutdown_tx,
        }
    }

    /// Create and start an imposter
    /// Returns the assigned port (which may have been auto-assigned if not specified)
    pub async fn create_imposter(&self, mut config: ImposterConfig) -> Result<u16, ImposterError> {
        // Validate protocol first
        match config.protocol.as_str() {
            "http" | "https" => {}
            proto => return Err(ImposterError::InvalidProtocol(proto.to_string())),
        }

        let bind_host: &str = config.host.as_deref().unwrap_or("0.0.0.0");
        // Determine port - either from config or auto-assign
        let (port, listener) = if let Some(p) = config.port {
            // Check if specified port is already in use
            if self.imposters.read().contains_key(&p) {
                return Err(ImposterError::PortInUse(p));
            }
            (
                p,
                TcpListener::bind((bind_host, p))
                    .await
                    .map_err(|e| ImposterError::BindError(p, e.to_string()))?,
            )
        } else {
            // Auto-assign port: find an available port starting from a base
            self.find_available_port(bind_host).await?
        };

        config.port = Some(port);

        info!("Imposter bound to {}:{}", bind_host, port);
        // Create imposter
        let mut imposter = Imposter::new(config);

        // Create shutdown channel for this imposter
        let (shutdown_tx, _) = broadcast::channel(1);
        imposter.shutdown_tx = Some(shutdown_tx.clone());

        let imposter = Arc::new(imposter);

        // Start serving
        let imposter_clone = Arc::clone(&imposter);
        let mut shutdown_rx = shutdown_tx.subscribe();

        let _handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                let imposter = Arc::clone(&imposter_clone);
                                tokio::spawn(async move {
                                    let io = TokioIo::new(stream);
                                    let service = service_fn(move |req| {
                                        let imposter = Arc::clone(&imposter);
                                        async move {
                                            handle_imposter_request(req, imposter, addr).await
                                        }
                                    });
                                    if let Err(e) = http1::Builder::new()
                                        .serve_connection(io, service)
                                        .await
                                    {
                                        debug!("Connection error on port {}: {}", port, e);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Accept error on port {}: {}", port, e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Imposter on port {} shutting down", port);
                        break;
                    }
                }
            }
        });

        // Store task handle (we need to work around the Arc)
        // Since we can't modify the Arc'd imposter, we'll track handles separately

        // Store imposter
        {
            let mut imposters = self.imposters.write();
            imposters.insert(port, imposter);
        }

        Ok(port)
    }

    /// Bind to an available port for auto-assignment
    /// Starts from port 49152 (start of dynamic/private port range) and finds first available
    async fn find_available_port(&self, host: &str) -> Result<(u16, TcpListener), ImposterError> {
        let existing_ports: std::collections::HashSet<u16> = {
            let imposters = self.imposters.read();
            imposters.keys().copied().collect()
        };

        // Start from dynamic port range (49152-65535)
        // If we could allow random ports, rather than requiring the minimum available port,
        // we could bind to port 0, and let the OS pick an unused ephemeral port for us.
        // Try ports in this range until we find one that's available
        for port in 49152..=u16::MAX {
            if existing_ports.contains(&port) {
                continue;
            }
            // Try to bind to check if OS has it available
            match TcpListener::bind((host, port)).await {
                Ok(listener) => {
                    // Port is available, return the port and bound listener
                    return Ok((port, listener));
                }
                Err(_) => continue, // Port in use by OS, try next
            }
        }

        Err(ImposterError::BindError(
            0,
            "No available ports in range 49152-65535".to_string(),
        ))
    }

    /// Delete an imposter
    pub async fn delete_imposter(&self, port: u16) -> Result<ImposterConfig, ImposterError> {
        let imposter = {
            let mut imposters = self.imposters.write();
            imposters
                .remove(&port)
                .ok_or(ImposterError::NotFound(port))?
        };

        // Send shutdown signal
        if let Some(ref tx) = imposter.shutdown_tx {
            let _ = tx.send(());
        }

        // Clear JavaScript inject state for this imposter
        #[cfg(feature = "javascript")]
        crate::scripting::clear_imposter_state(port);

        info!("Imposter on port {} deleted", port);
        Ok(imposter.config.clone())
    }

    /// Get an imposter by port
    pub fn get_imposter(&self, port: u16) -> Result<Arc<Imposter>, ImposterError> {
        let imposters = self.imposters.read();
        imposters
            .get(&port)
            .cloned()
            .ok_or(ImposterError::NotFound(port))
    }

    /// List all imposters
    pub fn list_imposters(&self) -> Vec<Arc<Imposter>> {
        let imposters = self.imposters.read();
        imposters.values().cloned().collect()
    }

    /// Delete all imposters
    pub async fn delete_all(&self) -> Vec<ImposterConfig> {
        let ports: Vec<u16> = {
            let imposters = self.imposters.read();
            imposters.keys().copied().collect()
        };

        let mut configs = Vec::new();
        for port in ports {
            if let Ok(config) = self.delete_imposter(port).await {
                configs.push(config);
            }
        }

        configs
    }

    /// Get imposter count (for future metrics)
    #[allow(dead_code)]
    pub fn count(&self) -> usize {
        self.imposters.read().len()
    }

    /// Add stub to an imposter
    pub fn add_stub(
        &self,
        port: u16,
        stub: Stub,
        index: Option<usize>,
    ) -> Result<(), ImposterError> {
        let imposter = self.get_imposter(port)?;
        imposter.add_stub(stub, index);
        Ok(())
    }

    /// Replace a stub
    pub fn replace_stub(&self, port: u16, index: usize, stub: Stub) -> Result<(), ImposterError> {
        let imposter = self.get_imposter(port)?;
        imposter
            .replace_stub(index, stub)
            .map_err(|_| ImposterError::StubIndexOutOfBounds(index))
    }

    /// Delete a stub
    pub fn delete_stub(&self, port: u16, index: usize) -> Result<(), ImposterError> {
        let imposter = self.get_imposter(port)?;
        imposter
            .delete_stub(index)
            .map_err(|_| ImposterError::StubIndexOutOfBounds(index))
    }

    /// Get a specific stub by index
    pub fn get_stub(&self, port: u16, index: usize) -> Result<Stub, ImposterError> {
        let imposter = self.get_imposter(port)?;
        imposter
            .get_stub(index)
            .ok_or(ImposterError::StubIndexOutOfBounds(index))
    }

    /// Shutdown all imposters (for future graceful shutdown)
    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
        self.delete_all().await;
    }
}

impl Default for ImposterManager {
    fn default() -> Self {
        Self::new()
    }
}
