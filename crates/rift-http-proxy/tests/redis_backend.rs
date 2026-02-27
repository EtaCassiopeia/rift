//! Integration tests for RedisFlowStore using testcontainers.
//!
//! These tests automatically start a Redis container, so no manual Docker setup is needed.
//! Requires Docker to be running.

#[cfg(feature = "redis-backend")]
mod tests {
    use rift_http_proxy::backends::RedisFlowStore;
    use rift_http_proxy::flow_state::FlowStore;
    use serde_json::json;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::redis::Redis;

    async fn setup(ttl: i64) -> (testcontainers::ContainerAsync<Redis>, RedisFlowStore) {
        let container = Redis::default().start().await.unwrap();
        let port = container.get_host_port_ipv4(6379).await.unwrap();
        let store = RedisFlowStore::new(
            &format!("redis://127.0.0.1:{port}"),
            5,
            "test:".to_string(),
            ttl,
        )
        .unwrap();
        (container, store)
    }

    #[tokio::test]
    async fn test_redis_get_set() {
        let (_container, store) = setup(300).await;

        store.set("flow1", "key1", json!("value1")).unwrap();
        let value = store.get("flow1", "key1").unwrap();
        assert_eq!(value, Some(json!("value1")));

        store.delete("flow1", "key1").unwrap();
    }

    #[tokio::test]
    async fn test_redis_increment() {
        let (_container, store) = setup(300).await;

        let v1 = store.increment("flow1", "counter").unwrap();
        assert_eq!(v1, 1);

        let v2 = store.increment("flow1", "counter").unwrap();
        assert_eq!(v2, 2);

        let v3 = store.increment("flow1", "counter").unwrap();
        assert_eq!(v3, 3);

        store.delete("flow1", "counter").unwrap();
    }

    #[tokio::test]
    async fn test_redis_ttl() {
        let (_container, store) = setup(2).await;

        store.set("flow1", "key1", json!("value1")).unwrap();
        assert!(store.exists("flow1", "key1").unwrap());

        // Wait for expiry
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        assert!(!store.exists("flow1", "key1").unwrap());
    }

    #[tokio::test]
    async fn test_redis_exists_delete() {
        let (_container, store) = setup(300).await;

        store.set("flow1", "key1", json!("value1")).unwrap();
        assert!(store.exists("flow1", "key1").unwrap());

        store.delete("flow1", "key1").unwrap();
        assert!(!store.exists("flow1", "key1").unwrap());
    }
}
