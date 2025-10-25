//! HTTP client helpers for making requests to both services

use crate::world::{CompatibilityWorld, DualResponse, Service};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

impl CompatibilityWorld {
    /// Send a request to both services and compare responses
    pub async fn send_to_both(
        &mut self,
        method: &str,
        path: &str,
        body: Option<&str>,
        headers: Option<&[(String, String)]>,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mb_url = format!("{}{}", self.config.mb_admin_url, path);
        let rift_url = format!("{}{}", self.config.rift_admin_url, path);

        let (mb_response, rift_response) = tokio::join!(
            self.send_request(&mb_url, method, body, headers),
            self.send_request(&rift_url, method, body, headers)
        );

        let mb = mb_response?;
        let rift = rift_response?;

        let dual = DualResponse {
            mb_status: mb.0,
            mb_body: mb.1,
            mb_headers: mb.2,
            mb_duration: mb.3,
            rift_status: rift.0,
            rift_body: rift.1,
            rift_headers: rift.2,
            rift_duration: rift.3,
        };

        self.last_response = Some(dual.clone());
        Ok(dual)
    }

    /// Send a request to imposter port on both services
    pub async fn send_to_imposter(
        &mut self,
        port: u16,
        method: &str,
        path: &str,
        body: Option<&str>,
        headers: Option<&[(String, String)]>,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mb_url = format!("{}{}", self.get_imposter_url(port, Service::Mountebank), path);
        let rift_url = format!("{}{}", self.get_imposter_url(port, Service::Rift), path);

        let (mb_response, rift_response) = tokio::join!(
            self.send_request(&mb_url, method, body, headers),
            self.send_request(&rift_url, method, body, headers)
        );

        let mb = mb_response?;
        let rift = rift_response?;

        let dual = DualResponse {
            mb_status: mb.0,
            mb_body: mb.1,
            mb_headers: mb.2,
            mb_duration: mb.3,
            rift_status: rift.0,
            rift_body: rift.1,
            rift_headers: rift.2,
            rift_duration: rift.3,
        };

        self.last_response = Some(dual.clone());
        self.response_sequence.push(dual.clone());
        Ok(dual)
    }

    /// Send a single HTTP request
    async fn send_request(
        &self,
        url: &str,
        method: &str,
        body: Option<&str>,
        headers: Option<&[(String, String)]>,
    ) -> Result<(u16, String, HashMap<String, String>, Duration), Box<dyn std::error::Error + Send + Sync>>
    {
        let start = Instant::now();

        let mut request = match method.to_uppercase().as_str() {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            "DELETE" => self.client.delete(url),
            "PATCH" => self.client.patch(url),
            "HEAD" => self.client.head(url),
            _ => return Err(format!("Unknown method: {}", method).into()),
        };

        if let Some(body) = body {
            request = request
                .header("Content-Type", "application/json")
                .body(body.to_string());
        }

        if let Some(hdrs) = headers {
            let mut header_map = HeaderMap::new();
            for (name, value) in hdrs {
                header_map.insert(
                    HeaderName::from_bytes(name.as_bytes())?,
                    HeaderValue::from_str(value)?,
                );
            }
            request = request.headers(header_map);
        }

        let response = request.send().await?;
        let duration = start.elapsed();
        let status = response.status().as_u16();

        let mut headers = HashMap::new();
        for (name, value) in response.headers() {
            if let Ok(v) = value.to_str() {
                headers.insert(name.to_string(), v.to_string());
            }
        }

        let body = response.text().await?;

        Ok((status, body, headers, duration))
    }

    /// Create an imposter on both services
    pub async fn create_imposter_on_both(
        &mut self,
        imposter_json: &str,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        self.send_to_both("POST", "/imposters", Some(imposter_json), None)
            .await
    }

    /// Get imposter details from both services
    pub async fn get_imposter_from_both(
        &mut self,
        port: u16,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        self.send_to_both("GET", &format!("/imposters/{}", port), None, None)
            .await
    }

    /// Add a stub to an imposter on both services
    pub async fn add_stub_to_both(
        &mut self,
        port: u16,
        stub_json: &str,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        let wrapped = format!(r#"{{"stub": {}}}"#, stub_json);
        self.send_to_both(
            "POST",
            &format!("/imposters/{}/stubs", port),
            Some(&wrapped),
            None,
        )
        .await
    }

    /// Replace a stub on both services
    pub async fn replace_stub_on_both(
        &mut self,
        port: u16,
        stub_index: usize,
        stub_json: &str,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        self.send_to_both(
            "PUT",
            &format!("/imposters/{}/stubs/{}", port, stub_index),
            Some(stub_json),
            None,
        )
        .await
    }

    /// Get recorded requests from both services
    pub async fn get_recorded_requests(
        &mut self,
        port: u16,
    ) -> Result<(Vec<Value>, Vec<Value>), Box<dyn std::error::Error + Send + Sync>> {
        let response = self.get_imposter_from_both(port).await?;

        let mb_json: Value = serde_json::from_str(&response.mb_body)?;
        let rift_json: Value = serde_json::from_str(&response.rift_body)?;

        let mb_requests = mb_json["requests"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let rift_requests = rift_json["requests"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        self.recorded_requests = Some((mb_requests.clone(), rift_requests.clone()));
        Ok((mb_requests, rift_requests))
    }

    /// Get request count from both services
    pub async fn get_request_count(
        &self,
        port: u16,
    ) -> Result<(u64, u64), Box<dyn std::error::Error + Send + Sync>> {
        let mb_url = format!("{}/imposters/{}", self.config.mb_admin_url, port);
        let rift_url = format!("{}/imposters/{}", self.config.rift_admin_url, port);

        let (mb_response, rift_response) = tokio::join!(
            self.client.get(&mb_url).send(),
            self.client.get(&rift_url).send()
        );

        let mb_body = mb_response?.text().await?;
        let rift_body = rift_response?.text().await?;

        let mb_json: Value = serde_json::from_str(&mb_body)?;
        let rift_json: Value = serde_json::from_str(&rift_body)?;

        let mb_count = mb_json["numberOfRequests"].as_u64().unwrap_or(0);
        let rift_count = rift_json["numberOfRequests"].as_u64().unwrap_or(0);

        Ok((mb_count, rift_count))
    }

    /// Clear response sequence
    pub fn clear_response_sequence(&mut self) {
        self.response_sequence.clear();
    }
}
