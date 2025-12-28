# Rift

**High-performance Mountebank-compatible HTTP/HTTPS mock server written in Rust**

[![Status](https://img.shields.io/badge/status-beta-blue)](https://github.com/EtaCassiopeia/rift)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)](https://www.rust-lang.org/)

Rift is a high-performance, [Mountebank](http://www.mbtest.org/)-compatible mock server that delivers **2-250x better performance**. Use your existing Mountebank configurations and enjoy faster test execution.

**[Documentation](https://etacassiopeia.github.io/rift/)** | **[Quick Start](#quick-start)** | **[Examples](examples/)**

---

## Why Rift?

### Mountebank Compatible

- **Same REST API** - Works with existing Mountebank clients and tooling
- **Same Configuration** - Load your `imposters.json` without changes
- **Same Behavior** - Predicates, responses, behaviors all work identically

### Blazing Fast Performance

| Feature | Mountebank | Rift | Speedup |
|:--------|:-----------|:-----|:--------|
| Simple stubs | 1,900 RPS | 39,000 RPS | **20x faster** |
| JSONPath predicates | 107 RPS | 26,500 RPS | **247x faster** |
| XPath predicates | 169 RPS | 28,700 RPS | **170x faster** |
| Complex predicates | 900 RPS | 29,300 RPS | **32x faster** |

### Full Feature Support

- **Imposters** - HTTP/HTTPS mock servers
- **Predicates** - equals, contains, matches, exists, jsonpath, xpath, and, or, not
- **Responses** - Static, proxy, injection
- **Behaviors** - wait, decorate, copy, lookup
- **Proxy Mode** - Record and replay

---

## Quick Start

### Run with Docker

```bash
# Pull and run
docker pull zainalpour/rift-proxy:latest
docker run -p 2525:2525 zainalpour/rift-proxy:latest

# Create your first imposter
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4545,
    "protocol": "http",
    "stubs": [{
      "predicates": [{ "equals": { "path": "/hello" } }],
      "responses": [{ "is": { "statusCode": 200, "body": "Hello, World!" } }]
    }]
  }'

# Test it
curl http://localhost:4545/hello
```

### Use Existing Mountebank Config

```bash
# Load your existing imposters.json
docker run -p 2525:2525 -v $(pwd)/imposters.json:/imposters.json \
  zainalpour/rift-proxy:latest --configfile /imposters.json
```

---

## Installation

### Docker (Recommended)

```bash
docker pull zainalpour/rift-proxy:latest
```

### Download Binary

```bash
# Linux
curl -L https://github.com/EtaCassiopeia/rift/releases/latest/download/rift-http-proxy-linux-x86_64 -o rift

# macOS (Apple Silicon)
curl -L https://github.com/EtaCassiopeia/rift/releases/latest/download/rift-http-proxy-darwin-aarch64 -o rift

# macOS (Intel)
curl -L https://github.com/EtaCassiopeia/rift/releases/latest/download/rift-http-proxy-darwin-x86_64 -o rift

chmod +x rift
./rift
```

### Build from Source

```bash
git clone https://github.com/EtaCassiopeia/rift.git
cd rift
cargo build --release
./target/release/rift-http-proxy
```

### Node.js / npm

For Node.js projects, use the official npm package:

```bash
npm install @rift-vs/rift
```

```javascript
import rift from '@rift-vs/rift';

const server = await rift.create({ port: 2525 });
// Create imposters, run tests...
await server.close();
```

---

## Documentation

### Getting Started
- [Installation](https://etacassiopeia.github.io/rift/getting-started/) - Docker, binary, build from source
- [Quick Start](https://etacassiopeia.github.io/rift/getting-started/quickstart/) - Create your first imposter
- [Node.js Integration](https://etacassiopeia.github.io/rift/getting-started/nodejs/) - npm package for Node.js
- [Migration Guide](https://etacassiopeia.github.io/rift/getting-started/migration/) - Using Rift with Mountebank configs

### Mountebank Compatibility
- [Imposters](https://etacassiopeia.github.io/rift/mountebank/imposters/) - Mock server configuration
- [Predicates](https://etacassiopeia.github.io/rift/mountebank/predicates/) - Request matching
- [Responses](https://etacassiopeia.github.io/rift/mountebank/responses/) - Response configuration
- [Behaviors](https://etacassiopeia.github.io/rift/mountebank/behaviors/) - wait, decorate, copy
- [Proxy Mode](https://etacassiopeia.github.io/rift/mountebank/proxy/) - Record and replay

### Configuration
- [Mountebank Format](https://etacassiopeia.github.io/rift/configuration/mountebank/) - JSON configuration
- [Native Rift Format](https://etacassiopeia.github.io/rift/configuration/native/) - YAML for advanced features
- [CLI Reference](https://etacassiopeia.github.io/rift/configuration/cli/) - Command-line options

### Features
- [Fault Injection](https://etacassiopeia.github.io/rift/features/fault-injection/) - Chaos engineering
- [Scripting](https://etacassiopeia.github.io/rift/features/scripting/) - Rhai, Lua, JavaScript
- [TLS/HTTPS](https://etacassiopeia.github.io/rift/features/tls/) - Secure connections
- [Metrics](https://etacassiopeia.github.io/rift/features/metrics/) - Prometheus integration
- [TUI](https://etacassiopeia.github.io/rift/features/tui/) - Interactive terminal interface

### Deployment
- [Docker](https://etacassiopeia.github.io/rift/deployment/docker/) - Container deployment
- [Kubernetes](https://etacassiopeia.github.io/rift/deployment/kubernetes/) - K8s patterns

### Reference
- [REST API](https://etacassiopeia.github.io/rift/api/) - Admin API reference
- [Performance](https://etacassiopeia.github.io/rift/performance/) - Benchmarks

---

## Example

```json
{
  "port": 4545,
  "protocol": "http",
  "name": "User Service",
  "stubs": [
    {
      "predicates": [{ "equals": { "method": "GET", "path": "/users" } }],
      "responses": [{
        "is": {
          "statusCode": 200,
          "headers": { "Content-Type": "application/json" },
          "body": [{ "id": 1, "name": "Alice" }]
        }
      }]
    },
    {
      "predicates": [{
        "and": [
          { "equals": { "method": "GET" } },
          { "matches": { "path": "/users/\\d+" } }
        ]
      }],
      "responses": [{
        "is": { "statusCode": 200, "body": { "id": 1, "name": "Alice" } }
      }]
    }
  ]
}
```

More examples in [`examples/`](examples/).

---

## Metrics

Prometheus metrics on `:9090/metrics`:

```bash
curl http://localhost:9090/metrics
```

Metrics include request counts, latency histograms, fault injection stats, and more.

---

## CLI Tools

Rift includes additional command-line tools:

### rift-tui - Interactive Terminal UI

Manage imposters and stubs through an interactive terminal interface:

```bash
# Build and run
cargo build --release --bin rift-tui
./target/release/rift-tui

# Connect to a different admin URL
./target/release/rift-tui --admin-url http://localhost:2525
```

Features:
- View and manage imposters with vim-style navigation (j/k)
- Create, edit, and delete stubs with JSON editor
- Generate curl commands for testing stubs
- Import/export imposter configurations
- Search and filter imposters and stubs
- Real-time metrics dashboard

### rift-verify - Stub Verification

Automatically test your imposters by generating requests from predicates:

```bash
cargo build --release --bin rift-verify
./target/release/rift-verify --show-curl
```

### rift-lint - Configuration Linter

Validate imposter configuration files before loading:

```bash
cargo build --release --bin rift-lint
./target/release/rift-lint ./imposters/
```

---

## Development

```bash
# Build
cargo build --release

# Run tests
cargo test --all

# Run with debug logging
RUST_LOG=debug ./target/release/rift-http-proxy

# Run benchmarks
cd tests/benchmark && ./scripts/run-benchmark.sh
```

---

## Contributing

Contributions welcome! Please read our contributing guidelines and submit PRs.

---

## License

Apache License 2.0 - see [LICENSE](LICENSE) for details.

---

## Acknowledgments

- [Mountebank](http://www.mbtest.org/) - The original service virtualization tool that inspired Rift's API
- [Tokio](https://tokio.rs/) - Async runtime for Rust
- [Hyper](https://hyper.rs/) - HTTP library for Rust
