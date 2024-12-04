use crate::behaviors::{
    apply_copy_behaviors, apply_decorate, apply_lookup_behaviors, apply_shell_transform, CsvCache,
    RequestContext, ResponseCycler,
};
use crate::config::{Config, Protocol as RiftProtocol, TcpFault, Upstream};
use crate::fault::{apply_latency, create_error_response, decide_fault, FaultDecision};
use crate::flow_state::{create_flow_store, FlowStore};
use crate::matcher::CompiledRule;
use crate::metrics;
use crate::recording::{ProxyMode, RecordedResponse, RecordingStore, RequestSignature};
use crate::routing::Router;
#[cfg(feature = "javascript")]
use crate::scripting::compile_js_to_bytecode;
#[cfg(feature = "lua")]
use crate::scripting::compile_to_bytecode;
use crate::scripting::RhaiEngine;
use crate::scripting::{
    CacheKey, CompiledScript, DecisionCache, DecisionCacheConfig,
    FaultDecision as ScriptFaultDecision, ScriptPool, ScriptPoolConfig, ScriptRequest,
};
use crate::template::{has_template_variables, process_template, RequestData};
#[cfg(any(feature = "lua", feature = "javascript"))]
use anyhow::Context;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::DigitallySignedStruct;
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, warn};

/// No-op certificate verifier for development/testing with self-signed certificates
/// WARNING: This disables all TLS security checks - use only in development!
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

/// Create a TCP listener with SO_REUSEPORT enabled for multi-worker setup
fn create_reusable_listener(addr: SocketAddr) -> std::io::Result<TcpListener> {
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };

    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;

    socket.set_reuse_address(true)?;

    // Set SO_REUSEPORT on Unix (macOS, Linux, BSD)
    // On macOS, SO_REUSEPORT is available but through setsockopt
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        unsafe {
            let optval: libc::c_int = 1;
            let ret = libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_REUSEPORT,
                &optval as *const _ as *const libc::c_void,
                std::mem::size_of_val(&optval) as libc::socklen_t,
            );
            if ret != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use std::os::fd::AsRawFd;
        unsafe {
            let optval: libc::c_int = 1;
            let ret = libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_REUSEPORT,
                &optval as *const _ as *const libc::c_void,
                std::mem::size_of_val(&optval) as libc::socklen_t,
            );
            if ret != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    socket.set_nonblocking(true)?;

    socket.bind(&addr.into())?;
    socket.listen(1024)?; // Backlog size

    // Convert to tokio TcpListener
    let std_listener: std::net::TcpListener = socket.into();
    TcpListener::from_std(std_listener)
}

/// Create TLS acceptor from certificate and key files
fn create_tls_acceptor(cert_path: &str, key_path: &str) -> Result<TlsAcceptor, anyhow::Error> {
    // Load certificate chain
    let cert_file = std::fs::File::open(cert_path)
        .map_err(|e| anyhow::anyhow!("Failed to open certificate file '{cert_path}': {e}"))?;
    let mut cert_reader = std::io::BufReader::new(cert_file);
    let certs: Vec<CertificateDer> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("Failed to parse certificate file: {e}"))?;

    if certs.is_empty() {
        anyhow::bail!("No certificates found in certificate file: {cert_path}");
    }

    // Load private key
    let key_file = std::fs::File::open(key_path)
        .map_err(|e| anyhow::anyhow!("Failed to open private key file '{key_path}': {e}"))?;
    let mut key_reader = std::io::BufReader::new(key_file);

    // Try reading as PKCS8, RSA, or EC private key
    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| anyhow::anyhow!("Failed to parse private key file: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("No private key found in key file: {key_path}"))?;

    // Build TLS server configuration
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| anyhow::anyhow!("Failed to build TLS configuration: {e}"))?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub struct ProxyServer {
    config: Arc<Config>,
    compiled_rules: Arc<Vec<CompiledRule>>,
    rule_upstreams: Arc<Vec<Option<String>>>, // Upstream filter for each rule (parallel to compiled_rules)
    upstream_uri: String,                     // Used for sidecar mode
    upstreams: Vec<Upstream>,                 // Used for reverse proxy mode
    router: Option<Router>,
    flow_store: Arc<dyn FlowStore>, // Flow store for scripts (may be NoOp if not configured)
    script_pool: Option<Arc<ScriptPool>>, // Script pool for optimized execution
    compiled_scripts: Option<Vec<(CompiledScript, crate::matcher::CompiledRule, Option<String>)>>, // Precompiled scripts for pool
    decision_cache: Option<Arc<DecisionCache>>, // Decision cache for memoization
    http_client: Client<
        hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        BoxBody<Bytes, hyper::Error>,
    >, // Shared HTTP client for HTTP/1.1
    // Mountebank-compatible behavior state
    #[allow(dead_code)] // Will be wired up when response cycling is fully integrated
    response_cycler: Arc<ResponseCycler>, // Response cycling state (repeat behavior)
    csv_cache: Arc<CsvCache>,             // CSV data cache (lookup behavior)
    recording_store: Arc<RecordingStore>, // Recording store (proxyOnce/proxyAlways modes)
}

impl ProxyServer {
    pub async fn new(config: Config) -> Result<Self, anyhow::Error> {
        Self::new_internal(config, None).await
    }

    pub async fn new_with_shared_flow_store(
        config: Config,
        flow_store: Arc<dyn FlowStore>,
    ) -> Result<Self, anyhow::Error> {
        Self::new_internal(config, Some(flow_store)).await
    }

    async fn new_internal(
        config: Config,
        shared_flow_store: Option<Arc<dyn FlowStore>>,
    ) -> Result<Self, anyhow::Error> {
        // Compile rules and extract upstream filters
        let mut compiled_rules = Vec::new();
        let mut rule_upstreams = Vec::new();

        for rule in &config.rules {
            compiled_rules.push(CompiledRule::compile(rule.clone())?);
            rule_upstreams.push(rule.upstream.clone());
        }

        // Get upstream URI (backward compatible with sidecar mode)
        let upstream_uri = if let Some(ref upstream) = config.upstream {
            let protocol = upstream.get_protocol();
            format!(
                "{}://{}:{}",
                protocol.as_str(),
                upstream.host,
                upstream.port
            )
        } else if !config.upstreams.is_empty() {
            // For reverse proxy mode, use first upstream as fallback
            config.upstreams[0].url.clone()
        } else {
            anyhow::bail!("Config must specify either 'upstream' (sidecar mode) or 'upstreams' (reverse proxy mode)");
        };

        // Create router for multi-upstream mode
        let router = if !config.routing.is_empty() {
            let r = Router::new(config.routing.clone())
                .map_err(|e| anyhow::anyhow!("Failed to create router: {e}"))?;
            Some(r)
        } else {
            None
        };

        // Use shared flow store if provided, otherwise initialize new one
        let flow_store: Arc<dyn FlowStore> = if let Some(store) = shared_flow_store {
            // Using shared flow store across workers
            store
        } else if let Some(ref fs_config) = config.flow_state {
            // Create new flow store for this worker (backward compatible mode)
            create_flow_store(fs_config)?
        } else if !config.script_rules.is_empty() {
            // Scripts are configured but no flow_state - use no-op store
            tracing::info!("Using NoOpFlowStore for scripts (flow_state not configured)");
            Arc::new(crate::flow_state::NoOpFlowStore)
        } else {
            // Neither scripts nor flow_state configured - use no-op store as placeholder
            Arc::new(crate::flow_state::NoOpFlowStore)
        };

        // Create script pool and decision cache for script execution
        let (script_pool, compiled_scripts, decision_cache) = if !config.script_rules.is_empty() {
            let mut scripts = Vec::new();
            let engine_type = config
                .script_engine
                .as_ref()
                .map(|cfg| cfg.engine.as_str())
                .unwrap_or("rhai");

            for script_rule in &config.script_rules {
                // Compile script to appropriate format
                let compiled = match engine_type {
                    "rhai" => {
                        let engine = RhaiEngine::new(&script_rule.script, script_rule.id.clone())?;
                        CompiledScript::Rhai {
                            ast: engine.ast().clone(),
                            rule_id: script_rule.id.clone(),
                        }
                    }
                    #[cfg(feature = "lua")]
                    "lua" => {
                        let bytecode =
                            compile_to_bytecode(&script_rule.script).with_context(|| {
                                format!(
                                    "Failed to compile Lua script for rule '{}'",
                                    script_rule.id
                                )
                            })?;
                        CompiledScript::Lua {
                            bytecode: Arc::new(bytecode),
                            rule_id: script_rule.id.clone(),
                        }
                    }
                    #[cfg(not(feature = "lua"))]
                    "lua" => {
                        anyhow::bail!("Lua engine not enabled. Enable the 'lua' feature flag")
                    }
                    #[cfg(feature = "javascript")]
                    "javascript" | "js" => {
                        let bytecode =
                            compile_js_to_bytecode(&script_rule.script).with_context(|| {
                                format!(
                                    "Failed to compile JavaScript script for rule '{}'",
                                    script_rule.id
                                )
                            })?;
                        CompiledScript::JavaScript {
                            bytecode: Arc::new(bytecode),
                            rule_id: script_rule.id.clone(),
                        }
                    }
                    #[cfg(not(feature = "javascript"))]
                    "javascript" | "js" => {
                        anyhow::bail!(
                            "JavaScript engine not enabled. Enable the 'javascript' feature flag"
                        )
                    }
                    other => anyhow::bail!("Unknown script engine type: {other}"),
                };

                let matcher = CompiledRule::compile(crate::config::Rule {
                    id: script_rule.id.clone(),
                    match_config: script_rule.match_config.clone(),
                    fault: Default::default(),
                    upstream: None,
                })?;

                scripts.push((compiled, matcher, script_rule.upstream.clone()));
            }

            // Create script pool with config (or defaults)
            let pool_config = if let Some(ref pool_cfg) = config.script_pool {
                ScriptPoolConfig {
                    workers: pool_cfg.workers,
                    queue_size: pool_cfg.queue_size,
                    timeout_ms: pool_cfg.timeout_ms,
                }
            } else {
                ScriptPoolConfig::default()
            };
            let pool = Arc::new(ScriptPool::new(pool_config.clone())?);
            info!(
                "Script pool initialized with {} workers",
                pool_config.workers
            );

            // Create decision cache with config (or defaults)
            let cache_config = if let Some(ref cache_cfg) = config.decision_cache {
                DecisionCacheConfig {
                    enabled: cache_cfg.enabled,
                    max_size: cache_cfg.max_size,
                    ttl_seconds: cache_cfg.ttl_seconds,
                }
            } else {
                DecisionCacheConfig::default()
            };
            let cache = Arc::new(DecisionCache::new(cache_config.clone()));
            info!(
                "Decision cache initialized: enabled={}, max_size={}, ttl={}s",
                cache_config.enabled, cache_config.max_size, cache_config.ttl_seconds
            );

            (Some(pool), Some(scripts), Some(cache))
        } else {
            (None, None, None)
        };

        let upstreams = config.upstreams.clone();

        // Check if any upstream needs TLS verification skipped
        let skip_tls_verify = upstreams.iter().any(|u| u.tls_skip_verify)
            || config
                .upstream
                .as_ref()
                .map(|u| u.tls_skip_verify)
                .unwrap_or(false);

        // Create shared HTTP client with HTTP/1.1 only
        let mut http_connector = hyper_util::client::legacy::connect::HttpConnector::new();
        http_connector.set_keepalive(Some(Duration::from_secs(
            config.connection_pool.keepalive_timeout_secs,
        )));
        http_connector.set_connect_timeout(Some(Duration::from_secs(
            config.connection_pool.connect_timeout_secs,
        )));
        http_connector.enforce_http(false); // Allow both HTTP and HTTPS

        // Build HTTPS connector for HTTP/1.1 only
        let https_connector = if skip_tls_verify {
            warn!("TLS certificate verification DISABLED for one or more upstreams (development/testing only)");
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_tls_config(
                    rustls::ClientConfig::builder()
                        .dangerous()
                        .with_custom_certificate_verifier(Arc::new(NoVerifier))
                        .with_no_client_auth(),
                )
                .https_or_http()
                .enable_http1()
                .wrap_connector(http_connector)
        } else {
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_native_roots()
                .expect("Failed to load native root certificates")
                .https_or_http()
                .enable_http1()
                .wrap_connector(http_connector)
        };

        let http_client = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(Duration::from_secs(
                config.connection_pool.idle_timeout_secs,
            ))
            .pool_max_idle_per_host(config.connection_pool.max_idle_per_host)
            .build(https_connector);

        info!(
            "Connection pool configured (HTTP/1.1): max_idle={}, idle_timeout={}s, keepalive={}s",
            config.connection_pool.max_idle_per_host,
            config.connection_pool.idle_timeout_secs,
            config.connection_pool.keepalive_timeout_secs
        );

        // Extract recording mode before moving config into Arc
        let recording_mode = config.recording.mode;

        Ok(Self {
            config: Arc::new(config),
            compiled_rules: Arc::new(compiled_rules),
            rule_upstreams: Arc::new(rule_upstreams),
            upstream_uri,
            upstreams,
            router,
            flow_store,
            script_pool,
            compiled_scripts,
            decision_cache,
            http_client,
            // Initialize behavior state
            response_cycler: Arc::new(ResponseCycler::new()),
            csv_cache: Arc::new(CsvCache::new()),
            recording_store: Arc::new(RecordingStore::new(recording_mode)),
        })
    }

    pub async fn run(self) -> Result<(), anyhow::Error> {
        let addr = SocketAddr::from(([0, 0, 0, 0], self.config.listen.port));
        let listener = create_reusable_listener(addr)?;
        let protocol = self.config.listen.protocol;

        // Create TLS acceptor if protocol is HTTPS
        let tls_acceptor = if protocol == RiftProtocol::Https {
            let tls_config =
                self.config.listen.tls.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("TLS configuration required for HTTPS listener")
                })?;
            Some(create_tls_acceptor(
                &tls_config.cert_path,
                &tls_config.key_path,
            )?)
        } else {
            None
        };

        info!("Listening on {}://{}", protocol.as_str(), addr);
        info!("Proxying to {}", self.upstream_uri);
        info!("Loaded {} fault injection rules", self.compiled_rules.len());
        if let Some(ref scripts) = self.compiled_scripts {
            info!("Loaded {} script rules", scripts.len());
        }
        if self.recording_store.mode() != ProxyMode::ProxyTransparent {
            info!("Recording mode: {:?}", self.recording_store.mode());
        }

        let server = Arc::new(self);

        loop {
            let (stream, remote_addr) = listener.accept().await?;
            let server = Arc::clone(&server);
            let tls_acceptor = tls_acceptor.clone();

            tokio::spawn(async move {
                match protocol {
                    RiftProtocol::Https => {
                        // HTTPS: perform TLS handshake first
                        let acceptor =
                            tls_acceptor.expect("TLS acceptor must be present for HTTPS");
                        match acceptor.accept(stream).await {
                            Ok(tls_stream) => {
                                let io = TokioIo::new(tls_stream);
                                let service = service_fn(move |req| {
                                    let server = Arc::clone(&server);
                                    async move { server.handle_request(req).await }
                                });

                                if let Err(err) =
                                    http1::Builder::new().serve_connection(io, service).await
                                {
                                    error!(
                                        "Error serving HTTPS connection from {}: {}",
                                        remote_addr, err
                                    );
                                }
                            }
                            Err(err) => {
                                error!("TLS handshake failed from {}: {}", remote_addr, err);
                            }
                        }
                    }
                    RiftProtocol::Http => {
                        // HTTP: serve directly
                        let io = TokioIo::new(stream);
                        let service = service_fn(move |req| {
                            let server = Arc::clone(&server);
                            async move { server.handle_request(req).await }
                        });

                        if let Err(err) = http1::Builder::new().serve_connection(io, service).await
                        {
                            error!(
                                "Error serving HTTP connection from {}: {}",
                                remote_addr, err
                            );
                        }
                    }
                    _ => {
                        error!("Unsupported protocol: {}", protocol.as_str());
                    }
                }
            });
        }
    }

    async fn handle_request(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Infallible> {
        let start_time = std::time::Instant::now();
        let method = req.method().clone();
        let uri = req.uri().clone();
        let headers = req.headers().clone();

        debug!("Received request: {} {}", method, uri);

        // Select upstream for this request (reverse proxy mode)
        let selected_upstream = self.select_upstream(&req);
        let (selected_upstream_url, selected_upstream_name) = match selected_upstream {
            Some((url, name)) => (Some(url), Some(name)),
            None => (None, None),
        };

        // Check script rules first (if configured) - optimized path with pool and cache
        if let (Some(ref compiled_scripts), Some(ref script_pool), Some(ref decision_cache)) = (
            &self.compiled_scripts,
            &self.script_pool,
            &self.decision_cache,
        ) {
            let flow_store = &self.flow_store;
            // Find first matching script rule that applies to selected upstream
            let matching_script =
                compiled_scripts
                    .iter()
                    .find(|(_, compiled_rule, rule_upstream)| {
                        compiled_rule.matches(&method, &uri, &headers)
                            && Self::rule_applies_to_upstream(
                                rule_upstream,
                                selected_upstream_name.as_deref(),
                            )
                    });

            if let Some((compiled_script, compiled_rule, _)) = matching_script {
                info!("Request matched script rule: {}", compiled_rule.id);

                // Collect body for script (needed for script context)
                let body_bytes = match req.collect().await {
                    Ok(collected) => collected.to_bytes(),
                    Err(e) => {
                        error!("Failed to collect request body: {}", e);
                        return Ok(error_response(500, "Failed to read request body")
                            .map(|b| BoxBody::new(b.map_err(|never| match never {}))));
                    }
                };

                // Convert to script request
                let mut headers_map = HashMap::new();
                for (k, v) in headers.iter() {
                    if let Ok(value_str) = v.to_str() {
                        headers_map.insert(k.as_str().to_string(), value_str.to_string());
                    }
                }

                let body_json: serde_json::Value =
                    serde_json::from_slice(&body_bytes).unwrap_or(serde_json::Value::Null);

                // Parse query parameters from URI
                let query_params = crate::predicate::parse_query_string(uri.query());

                let script_request = ScriptRequest {
                    method: method.to_string(),
                    path: uri.path().to_string(),
                    headers: headers_map.clone(),
                    body: body_json.clone(),
                    query: query_params,
                    path_params: std::collections::HashMap::new(), // TODO: Extract from route pattern if available
                };

                // Create cache key
                let cache_key = CacheKey::new(
                    method.to_string(),
                    uri.path().to_string(),
                    headers_map.into_iter().collect(),
                    &body_json,
                    compiled_rule.id.clone(),
                );

                // Determine if caching should be used
                // If flow_state is configured (not NoOpFlowStore), disable caching
                // because scripts using flow_store are stateful and results vary
                let use_cache = self.config.flow_state.is_none();

                // Check cache first (only for stateless scripts), then execute via pool
                let script_start = std::time::Instant::now();
                let result = if use_cache {
                    if let Some(cached_decision) = decision_cache.get(&cache_key) {
                        debug!("Cache hit for rule: {} (stateless)", compiled_rule.id);
                        Ok(cached_decision)
                    } else {
                        debug!("Cache miss for rule: {}", compiled_rule.id);

                        // Execute via pool
                        let pool_result = script_pool
                            .execute(
                                compiled_script.clone(),
                                script_request,
                                Arc::clone(flow_store),
                            )
                            .await;

                        // Cache the result if successful (stateless only)
                        if let Ok(ref decision) = pool_result {
                            let _ = decision_cache.insert(cache_key, decision.clone());
                        }

                        pool_result
                    }
                } else {
                    // Stateful script: always execute, never cache
                    debug!("Executing stateful script (no cache): {}", compiled_rule.id);
                    script_pool
                        .execute(
                            compiled_script.clone(),
                            script_request,
                            Arc::clone(flow_store),
                        )
                        .await
                };
                let script_duration = script_start.elapsed().as_secs_f64() * 1000.0;

                return match result {
                    Ok(ScriptFaultDecision::Error {
                        status,
                        body,
                        rule_id,
                        headers: script_headers,
                    }) => {
                        warn!(
                            "Script injecting error fault: status={}, rule={}",
                            status, rule_id
                        );

                        // Record metrics
                        metrics::record_script_execution(&rule_id, script_duration, "inject");
                        metrics::record_script_fault("error", &rule_id, None);
                        metrics::record_error_injection(&rule_id, status);

                        let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                        metrics::record_proxy_duration(method.as_str(), duration_ms, "script");
                        metrics::record_request(method.as_str(), status);

                        // Find fixed headers from matching YAML rule (if any)
                        let fixed_headers = self
                            .compiled_rules
                            .iter()
                            .enumerate()
                            .find(|(idx, rule)| {
                                rule.matches(&method, &uri, &headers)
                                    && Self::rule_applies_to_upstream(
                                        &self.rule_upstreams[*idx],
                                        selected_upstream_name.as_deref(),
                                    )
                                    && rule.rule.fault.error.is_some()
                            })
                            .and_then(|(_, rule)| {
                                rule.rule.fault.error.as_ref().map(|e| e.headers.clone())
                            });

                        let mut response = create_error_response(
                            status,
                            body,
                            fixed_headers.as_ref(),
                            Some(&script_headers),
                        )
                        .unwrap();
                        response
                            .headers_mut()
                            .insert("x-rift-fault", "error".parse().unwrap());
                        response
                            .headers_mut()
                            .insert("x-rift-rule-id", rule_id.parse().unwrap());
                        response
                            .headers_mut()
                            .insert("x-rift-script", "true".parse().unwrap());
                        Ok(response.map(|b| BoxBody::new(b.map_err(|never| match never {}))))
                    }
                    Ok(ScriptFaultDecision::Latency {
                        duration_ms,
                        rule_id,
                    }) => {
                        info!(
                            "Script injecting latency fault: {}ms, rule={}",
                            duration_ms, rule_id
                        );

                        // Record metrics
                        metrics::record_script_execution(&rule_id, script_duration, "inject");
                        metrics::record_script_fault("latency", &rule_id, Some(duration_ms));

                        apply_latency(duration_ms).await;

                        // Forward with body for latency fault
                        let mut response = self
                            .forward_request_with_body(
                                method.clone(),
                                uri.clone(),
                                headers.clone(),
                                body_bytes,
                                selected_upstream_url.as_deref(),
                            )
                            .await;
                        let status = response.status().as_u16();

                        let total_duration = start_time.elapsed().as_secs_f64() * 1000.0;
                        metrics::record_proxy_duration(method.as_str(), total_duration, "script");
                        metrics::record_request(method.as_str(), status);

                        response
                            .headers_mut()
                            .insert("x-rift-fault", "latency".parse().unwrap());
                        response
                            .headers_mut()
                            .insert("x-rift-rule-id", rule_id.parse().unwrap());
                        response
                            .headers_mut()
                            .insert("x-rift-script", "true".parse().unwrap());
                        response.headers_mut().insert(
                            "x-rift-latency-ms",
                            duration_ms.to_string().parse().unwrap(),
                        );
                        Ok(response.map(|b| BoxBody::new(b.map_err(|never| match never {}))))
                    }
                    Ok(ScriptFaultDecision::None) => {
                        debug!(
                            "Script decided not to inject fault for rule: {}",
                            compiled_rule.id
                        );
                        metrics::record_script_execution(
                            &compiled_rule.id,
                            script_duration,
                            "pass",
                        );

                        // Forward request
                        let response = self
                            .forward_request_with_body(
                                method.clone(),
                                uri.clone(),
                                headers.clone(),
                                body_bytes,
                                selected_upstream_url.as_deref(),
                            )
                            .await;
                        let status = response.status().as_u16();
                        let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                        metrics::record_proxy_duration(method.as_str(), duration_ms, "none");
                        metrics::record_request(method.as_str(), status);
                        Ok(response.map(|b| BoxBody::new(b.map_err(|never| match never {}))))
                    }
                    Err(e) => {
                        error!(
                            "Script execution error for rule {}: {}",
                            compiled_rule.id, e
                        );
                        metrics::record_script_execution(
                            &compiled_rule.id,
                            script_duration,
                            "error",
                        );
                        metrics::record_script_error(&compiled_rule.id, "runtime");

                        // Forward request on error
                        let response = self
                            .forward_request_with_body(
                                method.clone(),
                                uri.clone(),
                                headers.clone(),
                                body_bytes,
                                selected_upstream_url.as_deref(),
                            )
                            .await;
                        let status = response.status().as_u16();
                        let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                        metrics::record_proxy_duration(method.as_str(), duration_ms, "none");
                        metrics::record_request(method.as_str(), status);
                        Ok(response.map(|b| BoxBody::new(b.map_err(|never| match never {}))))
                    }
                };
            }
        }

        // Find matching rule (fallback to v1 rules) that applies to selected upstream
        let matched_rule_index = self
            .compiled_rules
            .iter()
            .enumerate()
            .find(|(idx, rule)| {
                rule.matches(&method, &uri, &headers)
                    && Self::rule_applies_to_upstream(
                        &self.rule_upstreams[*idx],
                        selected_upstream_name.as_deref(),
                    )
            })
            .map(|(idx, _)| idx);

        if let Some(rule_idx) = matched_rule_index {
            let rule = &self.compiled_rules[rule_idx];
            info!("Request matched rule: {}", rule.id);

            // Decide fault
            let fault_decision = decide_fault(&rule.rule.fault, &rule.id);

            match fault_decision {
                FaultDecision::TcpFault {
                    fault_type,
                    rule_id,
                } => {
                    warn!("Injecting TCP fault: {:?}, rule={}", fault_type, rule_id);

                    // Record metrics
                    metrics::record_error_injection(&rule_id, 0);
                    let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                    metrics::record_proxy_duration(method.as_str(), duration_ms, "tcp_fault");

                    // Return appropriate error based on fault type
                    let (status, body) = match fault_type {
                        TcpFault::ConnectionResetByPeer => {
                            (502, r#"{"error": "Connection reset by peer"}"#.to_string())
                        }
                        TcpFault::RandomDataThenClose => (
                            502,
                            r#"{"error": "Connection closed unexpectedly"}"#.to_string(),
                        ),
                    };

                    let mut response = create_error_response(status, body, None, None).unwrap();
                    response
                        .headers_mut()
                        .insert("x-rift-fault", "tcp".parse().unwrap());
                    response
                        .headers_mut()
                        .insert("x-rift-rule-id", rule_id.parse().unwrap());
                    response.headers_mut().insert(
                        "x-rift-tcp-fault",
                        format!("{fault_type:?}").to_lowercase().parse().unwrap(),
                    );
                    return Ok(response.map(|b| BoxBody::new(b.map_err(|never| match never {}))));
                }
                FaultDecision::Error {
                    status,
                    body,
                    rule_id,
                    headers: fault_headers,
                    behaviors,
                } => {
                    warn!("Injecting error fault: status={}, rule={}", status, rule_id);

                    // Apply wait behavior if present (Mountebank-compatible)
                    if let Some(ref bhvs) = behaviors {
                        if let Some(ref wait) = bhvs.wait {
                            let wait_ms = wait.get_duration_ms();
                            debug!("Applying wait behavior: {}ms", wait_ms);
                            apply_latency(wait_ms).await;
                        }
                    }

                    // Record metrics
                    metrics::record_error_injection(&rule_id, status);
                    let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                    metrics::record_proxy_duration(method.as_str(), duration_ms, "error");
                    metrics::record_request(method.as_str(), status);

                    // Build request context for behaviors
                    let request_context = RequestContext::from_request(
                        method.as_str(),
                        &uri,
                        &headers,
                        None, // Body not available for YAML rules
                    );

                    // Process template variables in response body if present
                    let mut processed_body = if has_template_variables(&body) {
                        let request_data = RequestData::new(
                            method.as_str(),
                            uri.path(),
                            uri.query(),
                            &headers,
                            None,
                        );
                        process_template(&body, &request_data)
                    } else {
                        body
                    };

                    // Clone headers for mutation
                    let mut response_headers = fault_headers.clone();

                    // Apply copy behaviors (Mountebank-compatible)
                    if let Some(ref bhvs) = behaviors {
                        if !bhvs.copy.is_empty() {
                            debug!("Applying {} copy behaviors", bhvs.copy.len());
                            processed_body = apply_copy_behaviors(
                                &processed_body,
                                &mut response_headers,
                                &bhvs.copy,
                                &request_context,
                            );
                        }
                    }

                    // Apply lookup behaviors (Mountebank-compatible)
                    if let Some(ref bhvs) = behaviors {
                        if !bhvs.lookup.is_empty() {
                            debug!("Applying {} lookup behaviors", bhvs.lookup.len());
                            processed_body = apply_lookup_behaviors(
                                &processed_body,
                                &mut response_headers,
                                &bhvs.lookup,
                                &request_context,
                                &self.csv_cache,
                            );
                        }
                    }

                    // Apply shell transform (Mountebank-compatible)
                    if let Some(ref bhvs) = behaviors {
                        if let Some(ref cmd) = bhvs.shell_transform {
                            debug!("Applying shell transform: {}", cmd);
                            match apply_shell_transform(
                                cmd,
                                &request_context,
                                &processed_body,
                                status,
                            ) {
                                Ok(transformed) => {
                                    processed_body = transformed;
                                }
                                Err(e) => {
                                    warn!("Shell transform failed: {}", e);
                                }
                            }
                        }
                    }

                    // Apply decorate behavior (Mountebank-compatible Rhai script)
                    let mut final_status = status;
                    if let Some(ref bhvs) = behaviors {
                        if let Some(ref script) = bhvs.decorate {
                            debug!("Applying decorate behavior");
                            match apply_decorate(
                                script,
                                &request_context,
                                &processed_body,
                                status,
                                &mut response_headers,
                            ) {
                                Ok((new_body, new_status)) => {
                                    processed_body = new_body;
                                    final_status = new_status;
                                }
                                Err(e) => {
                                    warn!("Decorate behavior failed: {}", e);
                                }
                            }
                        }
                    }

                    let mut response = create_error_response(
                        final_status,
                        processed_body,
                        Some(&response_headers),
                        None,
                    )
                    .unwrap();
                    response
                        .headers_mut()
                        .insert("x-rift-fault", "error".parse().unwrap());
                    response
                        .headers_mut()
                        .insert("x-rift-rule-id", rule_id.parse().unwrap());

                    // Add behavior headers for debugging/testing
                    if let Some(ref bhvs) = behaviors {
                        if bhvs.wait.is_some() {
                            response
                                .headers_mut()
                                .insert("x-rift-behavior-wait", "true".parse().unwrap());
                        }
                        if !bhvs.copy.is_empty() {
                            response
                                .headers_mut()
                                .insert("x-rift-behavior-copy", "true".parse().unwrap());
                        }
                        if !bhvs.lookup.is_empty() {
                            response
                                .headers_mut()
                                .insert("x-rift-behavior-lookup", "true".parse().unwrap());
                        }
                        if bhvs.shell_transform.is_some() {
                            response
                                .headers_mut()
                                .insert("x-rift-behavior-shell", "true".parse().unwrap());
                        }
                        if bhvs.decorate.is_some() {
                            response
                                .headers_mut()
                                .insert("x-rift-behavior-decorate", "true".parse().unwrap());
                        }
                    }

                    return Ok(response.map(|b| BoxBody::new(b.map_err(|never| match never {}))));
                }
                FaultDecision::Latency {
                    duration_ms,
                    rule_id,
                } => {
                    info!(
                        "Injecting latency fault: {}ms, rule={}",
                        duration_ms, rule_id
                    );

                    // Record metrics
                    metrics::record_latency_injection(&rule_id, duration_ms);

                    apply_latency(duration_ms).await;

                    // Collect body for retry capability
                    let body_bytes = match req.collect().await {
                        Ok(collected) => collected.to_bytes(),
                        Err(e) => {
                            error!("Failed to collect request body: {}", e);
                            let mut response = error_response(500, "Failed to read request body");
                            response
                                .headers_mut()
                                .insert("x-rift-fault", "latency".parse().unwrap());
                            response
                                .headers_mut()
                                .insert("x-rift-rule-id", rule_id.parse().unwrap());
                            return Ok(
                                response.map(|b| BoxBody::new(b.map_err(|never| match never {})))
                            );
                        }
                    };

                    // Forward request with latency header
                    let mut response = self
                        .forward_request_with_body(
                            method.clone(),
                            uri.clone(),
                            headers.clone(),
                            body_bytes,
                            selected_upstream_url.as_deref(),
                        )
                        .await;
                    let status = response.status().as_u16();
                    let total_duration = start_time.elapsed().as_secs_f64() * 1000.0;
                    metrics::record_proxy_duration(method.as_str(), total_duration, "latency");
                    metrics::record_request(method.as_str(), status);

                    response
                        .headers_mut()
                        .insert("x-rift-fault", "latency".parse().unwrap());
                    response
                        .headers_mut()
                        .insert("x-rift-rule-id", rule_id.parse().unwrap());
                    response.headers_mut().insert(
                        "x-rift-latency-ms",
                        duration_ms.to_string().parse().unwrap(),
                    );
                    return Ok(response.map(|b| BoxBody::new(b.map_err(|never| match never {}))));
                }
                FaultDecision::None => {
                    debug!("No fault injected for matched rule: {}", rule.id);
                }
            }
        }

        // Forward request without fault (with recording support if enabled)
        let response = self
            .forward_with_recording(req, selected_upstream_url.as_deref())
            .await;
        let status = response.status().as_u16();
        let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
        metrics::record_proxy_duration(method.as_str(), duration_ms, "none");
        metrics::record_request(method.as_str(), status);
        Ok(response)
    }

    async fn forward_request_with_body(
        &self,
        method: hyper::Method,
        uri: hyper::Uri,
        headers: hyper::HeaderMap,
        body_bytes: Bytes,
        selected_upstream: Option<&str>,
    ) -> Response<Full<Bytes>> {
        // Use shared HTTP client with connection pooling

        // Build upstream URI (use selected upstream or default)
        let base_upstream = selected_upstream.unwrap_or(&self.upstream_uri);
        let upstream_path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
        let upstream_uri = format!("{base_upstream}{upstream_path}");

        debug!("Forwarding to: {}", upstream_uri);

        // Create new request to upstream
        let mut upstream_req = Request::builder().method(method).uri(upstream_uri);

        // Copy headers (skip host)
        for (key, value) in headers.iter() {
            if key != "host" {
                upstream_req = upstream_req.header(key, value);
            }
        }

        let upstream_req = upstream_req
            .body(BoxBody::new(
                Full::new(body_bytes).map_err(|never: Infallible| match never {}),
            ))
            .unwrap();

        match self.http_client.request(upstream_req).await {
            Ok(upstream_response) => {
                let (parts, body) = upstream_response.into_parts();
                let body_bytes = match body.collect().await {
                    Ok(collected) => collected.to_bytes(),
                    Err(e) => {
                        error!("Failed to collect upstream response body: {}", e);
                        return error_response(502, "Failed to read upstream response");
                    }
                };
                let mut response = Response::from_parts(parts, Full::new(body_bytes));
                response
                    .headers_mut()
                    .insert("x-rift-proxied", "true".parse().unwrap());
                response
            }
            Err(e) => {
                error!("Failed to forward request to upstream: {}", e);
                error_response(502, "Bad Gateway")
            }
        }
    }

    async fn forward_request_streaming(
        &self,
        req: Request<hyper::body::Incoming>,
        selected_upstream: Option<&str>,
    ) -> Response<BoxBody<Bytes, hyper::Error>> {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let headers = req.headers().clone();

        // Build upstream URI
        let base_upstream = selected_upstream.unwrap_or(&self.upstream_uri);
        let upstream_path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
        let upstream_uri = format!("{base_upstream}{upstream_path}");

        debug!("Forwarding (streaming) to: {}", upstream_uri);

        // Create upstream request with streaming body (no collect!)
        let mut upstream_req = Request::builder().method(method).uri(upstream_uri);

        // Copy headers (skip host)
        for (key, value) in headers.iter() {
            if key != "host" {
                upstream_req = upstream_req.header(key, value);
            }
        }

        // Pass request body through directly without buffering
        let upstream_req = upstream_req.body(BoxBody::new(req.into_body())).unwrap();

        // Forward with streaming response
        match self.http_client.request(upstream_req).await {
            Ok(upstream_response) => {
                let (mut parts, body) = upstream_response.into_parts();
                parts
                    .headers
                    .insert("x-rift-proxied", "true".parse().unwrap());
                Response::from_parts(parts, BoxBody::new(body))
            }
            Err(e) => {
                error!("Failed to forward request to upstream: {}", e);
                Response::builder()
                    .status(502)
                    .header("content-type", "application/json")
                    .body(BoxBody::new(
                        Full::new(Bytes::from(r#"{"error": "Bad Gateway"}"#))
                            .map_err(|never: Infallible| match never {}),
                    ))
                    .unwrap()
            }
        }
    }

    #[allow(dead_code)]
    async fn forward_request(
        &self,
        req: Request<hyper::body::Incoming>,
        selected_upstream: Option<&str>,
    ) -> Response<Full<Bytes>> {
        // Use shared HTTP client with connection pooling

        let method = req.method().clone();
        let uri = req.uri().clone();

        // Build upstream URI (use selected upstream or default)
        let base_upstream = selected_upstream.unwrap_or(&self.upstream_uri);
        let upstream_path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
        let upstream_uri = format!("{base_upstream}{upstream_path}");

        debug!("Forwarding to: {}", upstream_uri);

        // Create new request to upstream
        let mut upstream_req = Request::builder().method(method).uri(upstream_uri);

        // Copy headers (skip host)
        for (key, value) in req.headers() {
            if key != "host" {
                upstream_req = upstream_req.header(key, value);
            }
        }

        // Collect body
        let body_bytes = match req.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) => {
                error!("Failed to collect request body: {}", e);
                return error_response(500, "Failed to read request body");
            }
        };

        let upstream_req = upstream_req
            .body(BoxBody::new(
                Full::new(body_bytes).map_err(|never: Infallible| match never {}),
            ))
            .unwrap();

        // Send request to upstream
        match self.http_client.request(upstream_req).await {
            Ok(upstream_response) => {
                let (parts, body) = upstream_response.into_parts();

                // Collect upstream response body
                let body_bytes = match body.collect().await {
                    Ok(collected) => collected.to_bytes(),
                    Err(e) => {
                        error!("Failed to collect upstream response body: {}", e);
                        return error_response(502, "Failed to read upstream response");
                    }
                };

                let mut response = Response::from_parts(parts, Full::new(body_bytes));
                response
                    .headers_mut()
                    .insert("x-rift-proxied", "true".parse().unwrap());
                response
            }
            Err(e) => {
                error!("Failed to forward request to upstream: {}", e);
                error_response(502, "Bad Gateway")
            }
        }
    }

    /// Forward request with recording support (Mountebank-compatible proxyOnce/proxyAlways)
    async fn forward_with_recording(
        &self,
        req: Request<hyper::body::Incoming>,
        selected_upstream: Option<&str>,
    ) -> Response<BoxBody<Bytes, hyper::Error>> {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let headers = req.headers().clone();

        // For recording modes, we need to collect the body to create a signature
        let mode = self.recording_store.mode();
        if mode == ProxyMode::ProxyTransparent {
            // Transparent mode - no recording, use streaming
            return self.forward_request_streaming(req, selected_upstream).await;
        }

        // Collect body for signature creation
        let body_bytes = match req.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) => {
                error!("Failed to collect request body for recording: {}", e);
                return Response::builder()
                    .status(500)
                    .body(BoxBody::new(
                        Full::new(Bytes::from(r#"{"error": "Failed to read request body"}"#))
                            .map_err(|never: Infallible| match never {}),
                    ))
                    .unwrap();
            }
        };

        // Extract headers for signature based on predicateGenerators config
        let signature_headers: Vec<(String, String)> = self
            .config
            .recording
            .predicate_generators
            .iter()
            .flat_map(|pg| pg.matches.headers.iter())
            .filter_map(|header_name| {
                headers
                    .get(header_name)
                    .and_then(|v| v.to_str().ok())
                    .map(|v| (header_name.clone(), v.to_string()))
            })
            .collect();

        // Create request signature for recording lookup
        let signature =
            RequestSignature::new(method.as_str(), uri.path(), uri.query(), &signature_headers);

        // Check if we should proxy or replay
        if !self.recording_store.should_proxy(&signature) {
            // Return recorded response (proxyOnce mode with existing recording)
            if let Some(recorded) = self.recording_store.get_recorded(&signature) {
                debug!(
                    "Replaying recorded response for {} {} (status: {})",
                    method,
                    uri.path(),
                    recorded.status
                );

                let mut response = Response::builder().status(recorded.status);

                // Restore recorded headers
                for (key, value) in &recorded.headers {
                    if let Ok(header_value) = value.parse::<hyper::header::HeaderValue>() {
                        response = response.header(key.as_str(), header_value);
                    }
                }

                // Add replay indicator header
                response = response.header("x-rift-replayed", "true");

                return response
                    .body(BoxBody::new(
                        Full::new(Bytes::from(recorded.body.clone()))
                            .map_err(|never: Infallible| match never {}),
                    ))
                    .unwrap();
            }
        }

        // Forward request and record response
        let start = std::time::Instant::now();
        let response = self
            .forward_request_with_body(
                method.clone(),
                uri.clone(),
                headers,
                body_bytes,
                selected_upstream,
            )
            .await;

        let latency_ms = start.elapsed().as_millis() as u64;

        // Record the response
        let status = response.status().as_u16();
        let (parts, body) = response.into_parts();

        // Extract body bytes for recording
        let body_bytes: Bytes = match body.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => Bytes::new(),
        };

        // Extract headers for recording
        let mut recorded_headers = std::collections::HashMap::new();
        for (key, value) in parts.headers.iter() {
            if let Ok(value_str) = value.to_str() {
                recorded_headers.insert(key.as_str().to_string(), value_str.to_string());
            }
        }

        // Record the response
        let recorded_response = RecordedResponse {
            status,
            headers: recorded_headers,
            body: body_bytes.to_vec(),
            latency_ms: Some(latency_ms),
            timestamp_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        self.recording_store
            .record(signature, recorded_response.clone());
        debug!(
            "Recorded response for {} {} (status: {}, latency: {}ms)",
            method,
            uri.path(),
            status,
            latency_ms
        );

        // Reconstruct response
        let mut response = Response::from_parts(parts, Full::new(body_bytes));
        response
            .headers_mut()
            .insert("x-rift-recorded", "true".parse().unwrap());

        response.map(|b| BoxBody::new(b.map_err(|never: Infallible| match never {})))
    }

    /// Select upstream for the request based on routing rules
    /// Returns the upstream URL and name if matched, None for sidecar mode
    fn select_upstream<B>(&self, req: &Request<B>) -> Option<(String, String)> {
        // If no router configured, use sidecar mode (return None)
        let router = self.router.as_ref()?;

        // Match request to an upstream name
        let upstream_name = router.match_request(req)?;

        // Find upstream by name
        let upstream = self.upstreams.iter().find(|u| u.name == upstream_name)?;
        debug!("Routed to upstream: {} ({})", upstream_name, upstream.url);
        Some((upstream.url.clone(), upstream_name.to_string()))
    }

    /// Check if a rule applies to the given upstream
    /// Returns true if:
    /// - Rule has no upstream filter (applies to all)
    /// - Rule's upstream matches the selected upstream name
    /// - No upstream is selected (sidecar mode - applies to all)
    fn rule_applies_to_upstream(
        rule_upstream_filter: &Option<String>,
        selected_upstream_name: Option<&str>,
    ) -> bool {
        match (rule_upstream_filter, selected_upstream_name) {
            // Rule has no filter - applies to all upstreams
            (None, _) => true,
            // No upstream selected (sidecar mode) - rule applies
            (Some(_), None) => true,
            // Both specified - must match
            (Some(rule_upstream), Some(selected)) => rule_upstream == selected,
        }
    }
}

fn error_response(status: u16, message: &str) -> Response<Full<Bytes>> {
    let body = format!(r#"{{"error": "{message}"}}"#);
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // Tests for error_response helper function
    // ============================================

    #[test]
    fn test_error_response_basic() {
        let response = error_response(500, "Internal Server Error");
        assert_eq!(response.status(), 500);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_error_response_400() {
        let response = error_response(400, "Bad Request");
        assert_eq!(response.status(), 400);
    }

    #[test]
    fn test_error_response_502() {
        let response = error_response(502, "Bad Gateway");
        assert_eq!(response.status(), 502);
    }

    #[test]
    fn test_error_response_404() {
        let response = error_response(404, "Not Found");
        assert_eq!(response.status(), 404);
    }

    #[test]
    fn test_error_response_503() {
        let response = error_response(503, "Service Unavailable");
        assert_eq!(response.status(), 503);
    }

    // ============================================
    // Tests for rule_applies_to_upstream logic
    // ============================================

    #[test]
    fn test_rule_applies_to_upstream_no_filter() {
        // Rule with no upstream filter should apply to all upstreams
        assert!(ProxyServer::rule_applies_to_upstream(&None, None));
        assert!(ProxyServer::rule_applies_to_upstream(
            &None,
            Some("backend-a")
        ));
        assert!(ProxyServer::rule_applies_to_upstream(
            &None,
            Some("backend-b")
        ));
    }

    #[test]
    fn test_rule_applies_to_upstream_sidecar_mode() {
        // Sidecar mode (no upstream selected) - rule should apply
        assert!(ProxyServer::rule_applies_to_upstream(
            &Some("backend-a".to_string()),
            None
        ));
        assert!(ProxyServer::rule_applies_to_upstream(
            &Some("backend-b".to_string()),
            None
        ));
    }

    #[test]
    fn test_rule_applies_to_upstream_matching() {
        // Rule upstream matches selected upstream
        assert!(ProxyServer::rule_applies_to_upstream(
            &Some("backend-a".to_string()),
            Some("backend-a")
        ));
    }

    #[test]
    fn test_rule_applies_to_upstream_non_matching() {
        // Rule upstream does NOT match selected upstream
        assert!(!ProxyServer::rule_applies_to_upstream(
            &Some("backend-a".to_string()),
            Some("backend-b")
        ));
        assert!(!ProxyServer::rule_applies_to_upstream(
            &Some("backend-x".to_string()),
            Some("backend-y")
        ));
    }

    #[test]
    fn test_rule_applies_to_upstream_empty_strings() {
        // Empty string cases
        assert!(ProxyServer::rule_applies_to_upstream(
            &Some("".to_string()),
            Some("")
        ));
        assert!(!ProxyServer::rule_applies_to_upstream(
            &Some("backend".to_string()),
            Some("")
        ));
    }

    // ============================================
    // Tests for NoVerifier (TLS)
    // ============================================

    #[test]
    fn test_no_verifier_supported_schemes() {
        let verifier = NoVerifier;
        let schemes = verifier.supported_verify_schemes();
        assert!(!schemes.is_empty());
        assert!(schemes.contains(&rustls::SignatureScheme::RSA_PKCS1_SHA256));
        assert!(schemes.contains(&rustls::SignatureScheme::ECDSA_NISTP256_SHA256));
        assert!(schemes.contains(&rustls::SignatureScheme::ED25519));
        assert!(schemes.contains(&rustls::SignatureScheme::RSA_PSS_SHA256));
    }

    // ============================================
    // Tests for ScriptPoolConfig defaults
    // ============================================

    #[test]
    fn test_script_pool_config_creation() {
        use crate::scripting::ScriptPoolConfig;
        let config = ScriptPoolConfig::default();
        assert!(config.workers >= 2);
        assert!(config.workers <= 16);
    }

    // ============================================
    // Tests for DecisionCacheConfig defaults
    // ============================================

    #[test]
    fn test_decision_cache_config_creation() {
        use crate::scripting::DecisionCacheConfig;
        let config = DecisionCacheConfig::default();
        assert!(config.enabled);
        assert!(config.max_size > 0);
        assert!(config.ttl_seconds > 0);
    }

    // ============================================
    // Tests for recording mode
    // ============================================

    mod recording_tests {
        use crate::recording::ProxyMode;

        #[test]
        fn test_proxy_mode_default() {
            let mode = ProxyMode::default();
            assert_eq!(mode, ProxyMode::ProxyTransparent);
        }

        #[test]
        fn test_proxy_mode_variants() {
            assert_ne!(ProxyMode::ProxyOnce, ProxyMode::ProxyAlways);
            assert_ne!(ProxyMode::ProxyOnce, ProxyMode::ProxyTransparent);
            assert_ne!(ProxyMode::ProxyAlways, ProxyMode::ProxyTransparent);
        }
    }

    // ============================================
    // Tests for Router integration
    // ============================================

    mod router_tests {
        use crate::config::{Route, RouteMatch};
        use crate::routing::Router;

        #[test]
        fn test_router_creation_empty() {
            let router = Router::new(vec![]);
            assert!(router.is_ok());
        }

        #[test]
        fn test_router_creation_with_rules() {
            let routes = vec![Route {
                name: "api-route".to_string(),
                upstream: "backend-a".to_string(),
                match_config: RouteMatch {
                    path_prefix: Some("/api".to_string()),
                    ..Default::default()
                },
            }];
            let router = Router::new(routes);
            assert!(router.is_ok());
        }

        #[test]
        fn test_router_path_prefix_matching() {
            let routes = vec![
                Route {
                    name: "v1-route".to_string(),
                    upstream: "backend-a".to_string(),
                    match_config: RouteMatch {
                        path_prefix: Some("/api/v1".to_string()),
                        ..Default::default()
                    },
                },
                Route {
                    name: "v2-route".to_string(),
                    upstream: "backend-b".to_string(),
                    match_config: RouteMatch {
                        path_prefix: Some("/api/v2".to_string()),
                        ..Default::default()
                    },
                },
            ];
            let router = Router::new(routes).unwrap();

            // Create test request
            let req = hyper::Request::builder()
                .uri("http://localhost/api/v1/users")
                .body(())
                .unwrap();
            let matched = router.match_request(&req);
            assert_eq!(matched, Some("backend-a"));

            let req2 = hyper::Request::builder()
                .uri("http://localhost/api/v2/items")
                .body(())
                .unwrap();
            let matched2 = router.match_request(&req2);
            assert_eq!(matched2, Some("backend-b"));
        }

        #[test]
        fn test_router_no_match() {
            let routes = vec![Route {
                name: "api-route".to_string(),
                upstream: "backend-a".to_string(),
                match_config: RouteMatch {
                    path_prefix: Some("/api".to_string()),
                    ..Default::default()
                },
            }];
            let router = Router::new(routes).unwrap();

            let req = hyper::Request::builder()
                .uri("http://localhost/other/path")
                .body(())
                .unwrap();
            let matched = router.match_request(&req);
            assert_eq!(matched, None);
        }

        #[test]
        fn test_router_exact_path_matching() {
            let routes = vec![Route {
                name: "exact-route".to_string(),
                upstream: "backend-exact".to_string(),
                match_config: RouteMatch {
                    path_exact: Some("/exact/path".to_string()),
                    ..Default::default()
                },
            }];
            let router = Router::new(routes).unwrap();

            let req = hyper::Request::builder()
                .uri("http://localhost/exact/path")
                .body(())
                .unwrap();
            assert_eq!(router.match_request(&req), Some("backend-exact"));

            let req2 = hyper::Request::builder()
                .uri("http://localhost/exact/path/extra")
                .body(())
                .unwrap();
            assert_eq!(router.match_request(&req2), None);
        }
    }

    // ============================================
    // Tests for CompiledRule matching
    // ============================================

    mod compiled_rule_tests {
        use crate::config::{FaultConfig, MatchConfig, PathMatch, Rule};
        use crate::matcher::CompiledRule;
        use hyper::{HeaderMap, Method, Uri};

        fn create_rule(path: PathMatch, methods: Vec<&str>) -> Rule {
            Rule {
                id: "test-rule".to_string(),
                match_config: MatchConfig {
                    methods: methods.iter().map(|m| m.to_string()).collect(),
                    path,
                    headers: vec![],
                    header_predicates: vec![],
                    query: vec![],
                    body: None,
                    case_sensitive: true,
                },
                fault: FaultConfig::default(),
                upstream: None,
            }
        }

        #[test]
        fn test_compiled_rule_any_path() {
            let rule = create_rule(PathMatch::Any, vec!["GET"]);
            let compiled = CompiledRule::compile(rule).unwrap();

            let uri: Uri = "http://localhost/any/path/here".parse().unwrap();
            let headers = HeaderMap::new();
            assert!(compiled.matches(&Method::GET, &uri, &headers));
        }

        #[test]
        fn test_compiled_rule_exact_path() {
            let rule = create_rule(
                PathMatch::Exact {
                    exact: "/exact/path".to_string(),
                },
                vec!["POST"],
            );
            let compiled = CompiledRule::compile(rule).unwrap();

            let uri: Uri = "http://localhost/exact/path".parse().unwrap();
            let headers = HeaderMap::new();
            assert!(compiled.matches(&Method::POST, &uri, &headers));

            let uri2: Uri = "http://localhost/exact/path/extra".parse().unwrap();
            assert!(!compiled.matches(&Method::POST, &uri2, &headers));
        }

        #[test]
        fn test_compiled_rule_prefix_path() {
            let rule = create_rule(
                PathMatch::Prefix {
                    prefix: "/api/".to_string(),
                },
                vec![],
            );
            let compiled = CompiledRule::compile(rule).unwrap();

            let uri1: Uri = "http://localhost/api/users".parse().unwrap();
            let uri2: Uri = "http://localhost/api/items/123".parse().unwrap();
            let uri3: Uri = "http://localhost/other".parse().unwrap();
            let headers = HeaderMap::new();

            assert!(compiled.matches(&Method::GET, &uri1, &headers));
            assert!(compiled.matches(&Method::GET, &uri2, &headers));
            assert!(!compiled.matches(&Method::GET, &uri3, &headers));
        }

        #[test]
        fn test_compiled_rule_regex_path() {
            let rule = create_rule(
                PathMatch::Regex {
                    regex: r"^/api/v\d+/.*".to_string(),
                },
                vec![],
            );
            let compiled = CompiledRule::compile(rule).unwrap();

            let uri1: Uri = "http://localhost/api/v1/users".parse().unwrap();
            let uri2: Uri = "http://localhost/api/v2/items".parse().unwrap();
            let uri3: Uri = "http://localhost/api/users".parse().unwrap();
            let headers = HeaderMap::new();

            assert!(compiled.matches(&Method::GET, &uri1, &headers));
            assert!(compiled.matches(&Method::GET, &uri2, &headers));
            assert!(!compiled.matches(&Method::GET, &uri3, &headers));
        }

        #[test]
        fn test_compiled_rule_contains_path() {
            let rule = create_rule(
                PathMatch::Contains {
                    contains: "admin".to_string(),
                },
                vec![],
            );
            let compiled = CompiledRule::compile(rule).unwrap();

            let uri1: Uri = "http://localhost/api/admin/users".parse().unwrap();
            let uri2: Uri = "http://localhost/admin".parse().unwrap();
            let uri3: Uri = "http://localhost/api/users".parse().unwrap();
            let headers = HeaderMap::new();

            assert!(compiled.matches(&Method::GET, &uri1, &headers));
            assert!(compiled.matches(&Method::GET, &uri2, &headers));
            assert!(!compiled.matches(&Method::GET, &uri3, &headers));
        }

        #[test]
        fn test_compiled_rule_ends_with_path() {
            let rule = create_rule(
                PathMatch::EndsWith {
                    ends_with: ".json".to_string(),
                },
                vec![],
            );
            let compiled = CompiledRule::compile(rule).unwrap();

            let uri1: Uri = "http://localhost/api/data.json".parse().unwrap();
            let uri2: Uri = "http://localhost/config.json".parse().unwrap();
            let uri3: Uri = "http://localhost/api/data.xml".parse().unwrap();
            let headers = HeaderMap::new();

            assert!(compiled.matches(&Method::GET, &uri1, &headers));
            assert!(compiled.matches(&Method::GET, &uri2, &headers));
            assert!(!compiled.matches(&Method::GET, &uri3, &headers));
        }

        #[test]
        fn test_compiled_rule_multiple_methods() {
            let rule = create_rule(PathMatch::Any, vec!["GET", "POST", "PUT"]);
            let compiled = CompiledRule::compile(rule).unwrap();

            let uri: Uri = "http://localhost/test".parse().unwrap();
            let headers = HeaderMap::new();

            assert!(compiled.matches(&Method::GET, &uri, &headers));
            assert!(compiled.matches(&Method::POST, &uri, &headers));
            assert!(compiled.matches(&Method::PUT, &uri, &headers));
            assert!(!compiled.matches(&Method::DELETE, &uri, &headers));
        }

        #[test]
        fn test_compiled_rule_empty_methods_matches_all() {
            let rule = create_rule(PathMatch::Any, vec![]);
            let compiled = CompiledRule::compile(rule).unwrap();

            let uri: Uri = "http://localhost/test".parse().unwrap();
            let headers = HeaderMap::new();

            // Empty methods list should match all methods
            assert!(compiled.matches(&Method::GET, &uri, &headers));
            assert!(compiled.matches(&Method::POST, &uri, &headers));
            assert!(compiled.matches(&Method::DELETE, &uri, &headers));
            assert!(compiled.matches(&Method::PATCH, &uri, &headers));
        }
    }

    // ============================================
    // Tests for FlowStore trait implementations
    // ============================================

    mod flow_store_tests {
        use crate::flow_state::{FlowStore, NoOpFlowStore};
        use serde_json::json;

        #[test]
        fn test_noop_flow_store_get() {
            let store = NoOpFlowStore;
            let result = store.get("flow-1", "key").unwrap();
            assert!(result.is_none());
        }

        #[test]
        fn test_noop_flow_store_set() {
            let store = NoOpFlowStore;
            let result = store.set("flow-1", "key", json!({"value": 42}));
            assert!(result.is_ok());
        }

        #[test]
        fn test_noop_flow_store_exists() {
            let store = NoOpFlowStore;
            let result = store.exists("flow-1", "key").unwrap();
            assert!(!result);
        }

        #[test]
        fn test_noop_flow_store_delete() {
            let store = NoOpFlowStore;
            let result = store.delete("flow-1", "key");
            assert!(result.is_ok());
        }

        #[test]
        fn test_noop_flow_store_increment() {
            let store = NoOpFlowStore;
            let result = store.increment("flow-1", "counter").unwrap();
            // NoOpFlowStore always returns 1 for increment
            assert_eq!(result, 1);
        }

        #[test]
        fn test_noop_flow_store_set_ttl() {
            let store = NoOpFlowStore;
            let result = store.set_ttl("flow-1", 3600);
            assert!(result.is_ok());
        }
    }

    // ============================================
    // Tests for ResponseCycler and CsvCache
    // ============================================

    mod behavior_state_tests {
        use crate::behaviors::{CsvCache, ResponseCycler};

        #[test]
        fn test_response_cycler_creation() {
            let cycler = ResponseCycler::new();
            // Just verify it can be created
            assert!(std::mem::size_of_val(&cycler) > 0);
        }

        #[test]
        fn test_csv_cache_creation() {
            let cache = CsvCache::new();
            // Just verify it can be created
            assert!(std::mem::size_of_val(&cache) > 0);
        }
    }

    // ============================================
    // Tests for RecordingStore
    // ============================================

    mod recording_store_tests {
        use crate::recording::{ProxyMode, RecordedResponse, RecordingStore, RequestSignature};
        use std::collections::HashMap;

        #[test]
        fn test_recording_store_transparent_mode() {
            let store = RecordingStore::new(ProxyMode::ProxyTransparent);
            assert_eq!(store.mode(), ProxyMode::ProxyTransparent);
        }

        #[test]
        fn test_recording_store_proxy_once_mode() {
            let store = RecordingStore::new(ProxyMode::ProxyOnce);
            assert_eq!(store.mode(), ProxyMode::ProxyOnce);
        }

        #[test]
        fn test_recording_store_proxy_always_mode() {
            let store = RecordingStore::new(ProxyMode::ProxyAlways);
            assert_eq!(store.mode(), ProxyMode::ProxyAlways);
        }

        #[test]
        fn test_request_signature_creation() {
            let sig = RequestSignature::new("GET", "/api/users", Some("page=1"), &[]);
            assert!(std::mem::size_of_val(&sig) > 0);
        }

        #[test]
        fn test_request_signature_with_headers() {
            let headers = vec![
                ("Authorization".to_string(), "Bearer token".to_string()),
                ("X-Custom".to_string(), "value".to_string()),
            ];
            let sig = RequestSignature::new("POST", "/api/data", None, &headers);
            assert!(std::mem::size_of_val(&sig) > 0);
        }

        #[test]
        fn test_recorded_response_creation() {
            let response = RecordedResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"test body".to_vec(),
                latency_ms: Some(50),
                timestamp_secs: 1234567890,
            };
            assert_eq!(response.status, 200);
            assert_eq!(response.body, b"test body".to_vec());
        }

        #[test]
        fn test_recording_store_record_and_get() {
            let store = RecordingStore::new(ProxyMode::ProxyOnce);
            let sig = RequestSignature::new("GET", "/test", None, &[]);

            let response = RecordedResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"response".to_vec(),
                latency_ms: Some(10),
                timestamp_secs: 0,
            };

            store.record(sig.clone(), response);
            let recorded = store.get_recorded(&sig);
            assert!(recorded.is_some());
            assert_eq!(recorded.unwrap().status, 200);
        }

        #[test]
        fn test_recording_store_should_proxy_transparent() {
            let store = RecordingStore::new(ProxyMode::ProxyTransparent);
            let sig = RequestSignature::new("GET", "/test", None, &[]);
            // Transparent mode always proxies
            assert!(store.should_proxy(&sig));
        }

        #[test]
        fn test_recording_store_should_proxy_always() {
            let store = RecordingStore::new(ProxyMode::ProxyAlways);
            let sig = RequestSignature::new("GET", "/test", None, &[]);
            // Always mode always proxies (records but still proxies)
            assert!(store.should_proxy(&sig));

            // Even after recording, it should still proxy
            let response = RecordedResponse {
                status: 200,
                headers: HashMap::new(),
                body: vec![],
                latency_ms: None,
                timestamp_secs: 0,
            };
            store.record(sig.clone(), response);
            assert!(store.should_proxy(&sig));
        }

        #[test]
        fn test_recording_store_should_proxy_once() {
            let store = RecordingStore::new(ProxyMode::ProxyOnce);
            let sig = RequestSignature::new("GET", "/unique", None, &[]);

            // First time, should proxy
            assert!(store.should_proxy(&sig));

            // Record a response
            let response = RecordedResponse {
                status: 200,
                headers: HashMap::new(),
                body: vec![],
                latency_ms: None,
                timestamp_secs: 0,
            };
            store.record(sig.clone(), response);

            // After recording, should NOT proxy (return cached)
            assert!(!store.should_proxy(&sig));
        }
    }
}
