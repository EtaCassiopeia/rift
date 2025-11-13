---
layout: default
title: Getting Started
nav_order: 2
has_children: true
permalink: /getting-started/
---

# Getting Started with Rift

Rift is a high-performance, Mountebank-compatible HTTP/HTTPS mock server. This guide will help you install Rift and create your first imposter.

---

## Installation

### Docker (Recommended)

The easiest way to run Rift is using Docker:

```bash
# Pull the latest image
docker pull zainalpour/rift-proxy:latest

# Run Rift on port 2525 (Mountebank-compatible admin port)
docker run -p 2525:2525 zainalpour/rift-proxy:latest
```

### Download Binary

Download pre-built binaries from the [releases page](https://github.com/EtaCassiopeia/rift/releases):

```bash
# Linux (x86_64)
curl -L https://github.com/EtaCassiopeia/rift/releases/latest/download/rift-http-proxy-linux-x86_64 -o rift
chmod +x rift
./rift

# macOS (Apple Silicon)
curl -L https://github.com/EtaCassiopeia/rift/releases/latest/download/rift-http-proxy-darwin-aarch64 -o rift
chmod +x rift
./rift

# macOS (Intel)
curl -L https://github.com/EtaCassiopeia/rift/releases/latest/download/rift-http-proxy-darwin-x86_64 -o rift
chmod +x rift
./rift
```

### Build from Source

Requires Rust 1.70+:

```bash
git clone https://github.com/EtaCassiopeia/rift.git
cd rift
cargo build --release
./target/release/rift-http-proxy
```

---

## Verify Installation

Once Rift is running, verify it's working:

```bash
# Check the admin API
curl http://localhost:2525/

# Expected response:
{
  "_links": {
    "imposters": { "href": "/imposters" },
    "config": { "href": "/config" },
    "logs": { "href": "/logs" }
  }
}
```

---

## Your First Imposter

Create a simple HTTP mock that responds to GET requests:

```bash
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4545,
    "protocol": "http",
    "name": "My First Imposter",
    "stubs": [{
      "predicates": [{
        "equals": {
          "method": "GET",
          "path": "/api/greeting"
        }
      }],
      "responses": [{
        "is": {
          "statusCode": 200,
          "headers": { "Content-Type": "application/json" },
          "body": { "message": "Hello from Rift!" }
        }
      }]
    }]
  }'
```

Test your imposter:

```bash
curl http://localhost:4545/api/greeting

# Response:
{"message":"Hello from Rift!"}
```

---

## Load Existing Configuration

If you have an existing Mountebank configuration file, load it directly:

```bash
# Using Docker
docker run -p 2525:2525 -v $(pwd)/imposters.json:/imposters.json \
  zainalpour/rift-proxy:latest --configfile /imposters.json

# Using binary
./rift --configfile imposters.json
```

Example `imposters.json`:

```json
{
  "imposters": [
    {
      "port": 4545,
      "protocol": "http",
      "stubs": [
        {
          "predicates": [{ "equals": { "path": "/users" } }],
          "responses": [{ "is": { "statusCode": 200, "body": "[]" } }]
        }
      ]
    }
  ]
}
```

---

## Next Steps

- [Quick Start Tutorial]({{ site.baseurl }}/getting-started/quickstart/) - Detailed walkthrough
- [Predicates Guide]({{ site.baseurl }}/mountebank/predicates/) - Request matching
- [Responses Guide]({{ site.baseurl }}/mountebank/responses/) - Response configuration
- [Migration Guide]({{ site.baseurl }}/getting-started/migration/) - Switching from Mountebank
