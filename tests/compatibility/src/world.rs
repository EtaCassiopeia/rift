//! Test world containing shared state for cucumber tests

use cucumber::World;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use tokio::sync::RwLock;

/// Port mapping between Mountebank and Rift
/// Mountebank uses ports as-is, Rift maps them to +1000
pub const PORT_OFFSET: u16 = 1000;

/// Configuration for the test environment
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// Mountebank admin API URL
    pub mb_admin_url: String,
    /// Rift admin API URL
    pub rift_admin_url: String,
    /// Base URL for Mountebank imposters
    pub mb_imposter_base: String,
    /// Base URL for Rift imposters
    pub rift_imposter_base: String,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            mb_admin_url: "http://localhost:2525".to_string(),
            rift_admin_url: "http://localhost:3525".to_string(),
            mb_imposter_base: "http://localhost".to_string(),
            rift_imposter_base: "http://localhost".to_string(),
        }
    }
}

/// Response from both services for comparison
#[derive(Debug, Clone)]
pub struct DualResponse {
    pub mb_status: u16,
    pub mb_body: String,
    pub mb_headers: HashMap<String, String>,
    pub mb_duration: Duration,
    pub rift_status: u16,
    pub rift_body: String,
    pub rift_headers: HashMap<String, String>,
    pub rift_duration: Duration,
}

impl DualResponse {
    pub fn statuses_match(&self) -> bool {
        self.mb_status == self.rift_status
    }

    pub fn bodies_match(&self) -> bool {
        // Try to normalize JSON for comparison
        let mb_json: Result<Value, _> = serde_json::from_str(&self.mb_body);
        let rift_json: Result<Value, _> = serde_json::from_str(&self.rift_body);

        match (mb_json, rift_json) {
            (Ok(mb), Ok(rift)) => mb == rift,
            _ => self.mb_body == self.rift_body,
        }
    }
}

/// The test world containing all shared state
#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct CompatibilityWorld {
    /// HTTP client for making requests
    #[world(default)]
    pub client: Client,

    /// Test configuration
    pub config: TestConfig,

    /// Last response from both services
    pub last_response: Option<DualResponse>,

    /// Sequence of responses (for cycling tests)
    pub response_sequence: Vec<DualResponse>,

    /// Recorded requests from both services
    pub recorded_requests: Option<(Vec<Value>, Vec<Value>)>,

    /// Container references (for cleanup)
    #[world(default)]
    pub containers: ContainerState,

    /// Whether containers are external (docker-compose) or managed
    pub external_containers: bool,

    /// Whether a connection error occurred (for fault injection tests)
    pub connection_error: bool,
}

#[derive(Debug, Default)]
pub struct ContainerState {
    pub mountebank: Option<Arc<RwLock<ContainerAsync<GenericImage>>>>,
    pub rift: Option<Arc<RwLock<ContainerAsync<GenericImage>>>>,
}

impl CompatibilityWorld {
    /// Create a new test world
    pub async fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        // Check if external containers are running
        let external = Self::check_external_containers(&client).await;

        let config = if external {
            TestConfig::default()
        } else {
            // Will be updated when containers start
            TestConfig::default()
        };

        Self {
            client,
            config,
            last_response: None,
            response_sequence: Vec::new(),
            recorded_requests: None,
            containers: ContainerState::default(),
            external_containers: external,
            connection_error: false,
        }
    }

    /// Check if external containers (from docker-compose) are running
    async fn check_external_containers(client: &Client) -> bool {
        let mb_check = client
            .get("http://localhost:2525/")
            .timeout(Duration::from_secs(2))
            .send()
            .await;
        let rift_check = client
            .get("http://localhost:3525/")
            .timeout(Duration::from_secs(2))
            .send()
            .await;

        mb_check.is_ok() && rift_check.is_ok()
    }

    /// Start containers if not using external ones
    pub async fn ensure_containers(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.external_containers {
            return Ok(());
        }

        // Start Mountebank container
        if self.containers.mountebank.is_none() {
            let mb_container = GenericImage::new("bbyars/mountebank", "2.9.1")
                .with_wait_for(testcontainers::core::WaitFor::message_on_stdout(
                    "now taking orders",
                ))
                .with_exposed_port(2525.into())
                .with_exposed_port(4545.into())
                .with_exposed_port(4546.into())
                .with_exposed_port(4547.into())
                .with_cmd(["mb", "start", "--allowInjection", "--loglevel", "info"])
                .start()
                .await?;

            let mb_port = mb_container.get_host_port_ipv4(2525).await?;
            self.config.mb_admin_url = format!("http://localhost:{}", mb_port);

            self.containers.mountebank = Some(Arc::new(RwLock::new(mb_container)));
        }

        // Start Rift container
        if self.containers.rift.is_none() {
            let rift_container = GenericImage::new("rift-http-proxy", "latest")
                .with_wait_for(testcontainers::core::WaitFor::message_on_stdout(
                    "Admin API",
                ))
                .with_exposed_port(2525.into())
                .with_exposed_port(4545.into())
                .with_exposed_port(4546.into())
                .with_exposed_port(4547.into())
                .with_env_var("MB_PORT", "2525")
                .with_env_var("MB_ALLOW_INJECTION", "true")
                .start()
                .await?;

            let rift_port = rift_container.get_host_port_ipv4(2525).await?;
            self.config.rift_admin_url = format!("http://localhost:{}", rift_port);

            self.containers.rift = Some(Arc::new(RwLock::new(rift_container)));
        }

        Ok(())
    }

    /// Clear all imposters from both services
    pub async fn clear_imposters(&self) -> Result<(), reqwest::Error> {
        self.client
            .delete(format!("{}/imposters", self.config.mb_admin_url))
            .send()
            .await?;
        self.client
            .delete(format!("{}/imposters", self.config.rift_admin_url))
            .send()
            .await?;
        Ok(())
    }

    /// Get imposter port for a service
    pub fn get_imposter_url(&self, port: u16, service: Service) -> String {
        match service {
            Service::Mountebank => {
                if self.external_containers {
                    format!("{}:{}", self.config.mb_imposter_base, port)
                } else {
                    // Would need to get mapped port from container
                    format!("{}:{}", self.config.mb_imposter_base, port)
                }
            }
            Service::Rift => {
                if self.external_containers {
                    format!("{}:{}", self.config.rift_imposter_base, port + PORT_OFFSET)
                } else {
                    format!("{}:{}", self.config.rift_imposter_base, port + PORT_OFFSET)
                }
            }
        }
    }
}

/// Which service to target
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Mountebank,
    Rift,
}
