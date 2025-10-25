//! Cucumber BDD compatibility tests for Mountebank and Rift
//!
//! Run with: cargo test --test compatibility
//!
//! Prerequisites:
//! - Docker Compose services running: `docker compose up -d`
//! - Or testcontainers will start services automatically

use cucumber::{writer, World, WriterExt};
use rift_compatibility_tests::world::CompatibilityWorld;
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Initialize tracing for debugging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Run cucumber tests with our custom world
    CompatibilityWorld::cucumber()
        .max_concurrent_scenarios(1) // Run scenarios sequentially for stability
        .before(|_feature, _rule, scenario, world| {
            Box::pin(async move {
                tracing::info!("Starting scenario: {}", scenario.name);
                // Ensure containers are ready
                if let Err(e) = world.ensure_containers().await {
                    tracing::warn!("Container setup warning: {}", e);
                }
            })
        })
        .after(|_feature, _rule, scenario, _ev, world| {
            Box::pin(async move {
                tracing::info!("Finished scenario: {}", scenario.name);
                // Clean up after each scenario
                if let Some(w) = world {
                    let _ = w.clear_imposters().await;
                }
            })
        })
        .with_writer(
            writer::Basic::stdout()
                .summarized()
                .assert_normalized(),
        )
        .filter_run("features/", |_, _, sc| {
            // Skip scenarios tagged with @skip
            !sc.tags.iter().any(|t| t == "skip")
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quick smoke test to verify test infrastructure
    #[tokio::test]
    async fn test_world_creation() {
        let world = CompatibilityWorld::new().await;
        assert!(!world.config.mb_admin_url.is_empty());
        assert!(!world.config.rift_admin_url.is_empty());
    }
}
