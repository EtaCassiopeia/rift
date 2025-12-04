//! HTTP client helpers for making requests to both services

use crate::world::{CompatibilityWorld, DualResponse, Service};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

impl CompatibilityWorld {
    /// Send a request to both services and compare responses
    /// Automatically adjusts port numbers in paths for Rift (adds PORT_OFFSET)
    pub async fn send_to_both(
        &mut self,
        method: &str,
        path: &str,
        body: Option<&str>,
        headers: Option<&[(String, String)]>,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mb_url = format!("{}{}", self.config.mb_admin_url, path);
        // Adjust port in path for Rift if path contains /imposters/{port}
        let rift_path = self.adjust_path_port_for_rift(path);
        let rift_url = format!("{}{}", self.config.rift_admin_url, rift_path);

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

    /// Adjust port number in path for Rift
    /// NOTE: With Docker, both services use the same port numbers, so no adjustment needed.
    fn adjust_path_port_for_rift(&self, path: &str) -> String {
        // Don't adjust - both services use the same port numbers for imposters
        path.to_string()
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

    /// Send a request to Mountebank only (for Mountebank-only tests)
    pub async fn send_to_mountebank(
        &mut self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, String), Box<dyn std::error::Error + Send + Sync>> {
        let mb_url = format!("{}{}", self.config.mb_admin_url, path);
        let result = self.send_request(&mb_url, method, body, None).await?;
        // Store in last_response with Mountebank data only (Rift fields will be 0/empty)
        self.last_response = Some(DualResponse {
            mb_status: result.0,
            mb_body: result.1.clone(),
            mb_headers: result.2,
            mb_duration: result.3,
            rift_status: 0,
            rift_body: String::new(),
            rift_headers: std::collections::HashMap::new(),
            rift_duration: std::time::Duration::ZERO,
        });
        Ok((result.0, result.1))
    }

    /// Create an imposter on both services
    /// Note: Both services create imposters at the SAME port (e.g., 4545).
    /// Docker port mapping handles the offset (host 5545 -> container 4545).
    pub async fn create_imposter_on_both(
        &mut self,
        imposter_json: &str,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Don't adjust port - both services create at the same port
        // Docker port mapping handles the offset for access

        let mb_url = format!("{}/imposters", self.config.mb_admin_url);
        let rift_url = format!("{}/imposters", self.config.rift_admin_url);

        let (mb_response, rift_response) = tokio::join!(
            self.send_request(&mb_url, "POST", Some(imposter_json), None),
            self.send_request(&rift_url, "POST", Some(imposter_json), None)
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

    /// Adjust all port numbers in a PUT /imposters body for Rift
    /// NOTE: With Docker, we don't adjust ports - both services create at the same ports.
    /// This function now just returns the JSON as-is.
    pub fn adjust_imposters_body_for_rift(&self, json: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Don't adjust ports - both services use the same port numbers
        // Docker port mapping handles the offset for access
        Ok(json.to_string())
    }

    /// Get imposter details from both services
    /// Note: For Rift, the port is adjusted by PORT_OFFSET
    pub async fn get_imposter_from_both(
        &mut self,
        port: u16,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        self.send_to_both_with_port_adjustment("GET", "/imposters/{port}", port, None, None)
            .await
    }

    /// Add a stub to an imposter on both services
    /// Note: For Rift, the port is adjusted by PORT_OFFSET
    pub async fn add_stub_to_both(
        &mut self,
        port: u16,
        stub_json: &str,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        let wrapped = format!(r#"{{"stub": {}}}"#, stub_json);
        self.send_to_both_with_port_adjustment(
            "POST",
            "/imposters/{port}/stubs",
            port,
            Some(&wrapped),
            None,
        )
        .await
    }

    /// Replace a stub on both services
    /// Note: For Rift, the port is adjusted by PORT_OFFSET
    pub async fn replace_stub_on_both(
        &mut self,
        port: u16,
        stub_index: usize,
        stub_json: &str,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        self.send_to_both_with_port_adjustment(
            "PUT",
            &format!("/imposters/{{port}}/stubs/{}", stub_index),
            port,
            Some(stub_json),
            None,
        )
        .await
    }

    /// Send request to both services with port replacement
    /// The path_template should contain {port} which will be replaced with the actual port
    /// NOTE: With Docker, both services use the same port numbers for imposters.
    async fn send_to_both_with_port_adjustment(
        &mut self,
        method: &str,
        path_template: &str,
        port: u16,
        body: Option<&str>,
        headers: Option<&[(String, String)]>,
    ) -> Result<DualResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Both services use the same port numbers for imposters
        let path = path_template.replace("{port}", &port.to_string());

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
    /// Note: Both services use the same port numbers for imposters.
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

    /// Send a request to Rift admin API only (for Rift-only tests)
    pub async fn send_to_rift(
        &mut self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, String), Box<dyn std::error::Error + Send + Sync>> {
        let rift_url = format!("{}{}", self.config.rift_admin_url, path);
        let result = self.send_request(&rift_url, method, body, None).await?;
        // Store in last_response with Rift data only (Mountebank fields will be 0/empty)
        self.last_response = Some(DualResponse {
            mb_status: 0,
            mb_body: String::new(),
            mb_headers: HashMap::new(),
            mb_duration: std::time::Duration::ZERO,
            rift_status: result.0,
            rift_body: result.1.clone(),
            rift_headers: result.2,
            rift_duration: result.3,
        });
        Ok((result.0, result.1))
    }

    /// Send a request to Rift imposter only (for Rift-only tests)
    pub async fn send_to_rift_imposter(
        &self,
        port: u16,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let rift_url = format!("{}{}", self.get_imposter_url(port, Service::Rift), path);
        let result = self.send_request(&rift_url, method, body, None).await?;
        Ok(result.1)
    }
}
