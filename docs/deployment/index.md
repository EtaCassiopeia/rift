---
layout: default
title: Deployment
nav_order: 6
has_children: true
permalink: /deployment/
---

# Deployment

Rift can be deployed in various environments, from local development to production Kubernetes clusters.

---

## Deployment Options

### Docker (Recommended)

Quick setup for development and testing:

```bash
docker pull ghcr.io/etacassiopeia/rift-proxy:latest
docker run -p 2525:2525 ghcr.io/etacassiopeia/rift-proxy:latest
```

[Full Docker Guide]({{ site.baseurl }}/deployment/docker/)

### Kubernetes

Production deployment with proper resource management:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: rift
spec:
  replicas: 3
  template:
    spec:
      containers:
        - name: rift
          image: ghcr.io/etacassiopeia/rift-proxy:latest
          ports:
            - containerPort: 2525
            - containerPort: 9090
```

[Full Kubernetes Guide]({{ site.baseurl }}/deployment/kubernetes/)

### Binary

Standalone deployment without containers:

```bash
# Download
curl -L https://github.com/EtaCassiopeia/rift/releases/latest/download/rift-http-proxy-linux-x86_64 -o rift

# Run
chmod +x rift
./rift --configfile imposters.json
```

---

## Deployment Patterns

### Standalone Mock Server

Single Rift instance serving all imposters:

```
┌─────────────┐     ┌─────────────┐
│  Test Suite │────▶│    Rift     │
└─────────────┘     │  (imposters)│
                    └─────────────┘
```

### Sidecar Pattern

One Rift per service for isolated fault injection:

```
┌─────────────────────────────┐
│           Pod               │
│  ┌─────────┐  ┌─────────┐  │
│  │  Rift   │◀─│   App   │  │
│  │(sidecar)│  │         │  │
│  └────┬────┘  └─────────┘  │
│       │                     │
│       ▼                     │
│  ┌─────────┐               │
│  │ Backend │               │
│  └─────────┘               │
└─────────────────────────────┘
```

### API Gateway Pattern

Rift as a reverse proxy routing to multiple services:

```
                    ┌─────────────┐
┌─────────┐        │    Rift     │        ┌─────────┐
│ Client  │───────▶│  (gateway)  │───────▶│Service A│
└─────────┘        │             │        └─────────┘
                   │             │        ┌─────────┐
                   │             │───────▶│Service B│
                   └─────────────┘        └─────────┘
```

---

## Environment Configuration

### Required Settings

| Setting | Description | Default |
|:--------|:------------|:--------|
| `MB_PORT` | Admin API port | `2525` |

### Optional Settings

| Setting | Description | Default |
|:--------|:------------|:--------|
| `MB_ALLOW_INJECTION` | Enable JavaScript | `false` |
| `RUST_LOG` | Log level | `info` |
| `RIFT_METRICS_PORT` | Metrics port | `9090` |

---

## Resource Requirements

### Minimum (Development)

- **CPU**: 0.5 cores
- **Memory**: 128MB
- **Storage**: 50MB (image)

### Recommended (Production)

- **CPU**: 2 cores
- **Memory**: 512MB
- **Storage**: 100MB

### High Throughput

- **CPU**: 4+ cores
- **Memory**: 1GB+
- **Storage**: 100MB

---

## Health Checks

### Admin API

```bash
curl http://localhost:2525/
```

### Metrics Endpoint

```bash
curl http://localhost:9090/metrics
```

### Kubernetes Probes

```yaml
livenessProbe:
  httpGet:
    path: /
    port: 2525
  initialDelaySeconds: 5
  periodSeconds: 10

readinessProbe:
  httpGet:
    path: /
    port: 2525
  initialDelaySeconds: 5
  periodSeconds: 5
```
