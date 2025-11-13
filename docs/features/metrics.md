---
layout: default
title: Metrics
parent: Features
nav_order: 4
---

# Prometheus Metrics

Rift exposes metrics in Prometheus format for monitoring and alerting.

---

## Enabling Metrics

### Default Configuration

Metrics are enabled by default on port 9090:

```bash
curl http://localhost:9090/metrics
```

### Custom Port

```bash
# Environment variable
RIFT_METRICS_PORT=8090 rift-http-proxy

# Docker
docker run -e RIFT_METRICS_PORT=8090 -p 8090:8090 zainalpour/rift-proxy:latest
```

### Disable Metrics

```bash
RIFT_METRICS_ENABLED=false rift-http-proxy
```

---

## Available Metrics

### Request Metrics

```prometheus
# Total requests processed
rift_http_requests_total{method="GET", path="/api/users", status="200"} 1234

# Request duration histogram
rift_http_request_duration_seconds_bucket{le="0.001"} 100
rift_http_request_duration_seconds_bucket{le="0.01"} 500
rift_http_request_duration_seconds_bucket{le="0.1"} 900
rift_http_request_duration_seconds_bucket{le="1"} 1000
rift_http_request_duration_seconds_sum 45.67
rift_http_request_duration_seconds_count 1000
```

### Fault Injection Metrics

```prometheus
# Faults injected
rift_faults_injected_total{type="latency", rule="api-latency"} 300
rift_faults_injected_total{type="error", rule="api-errors"} 50

# Injected latency histogram
rift_injected_latency_seconds_bucket{le="0.1"} 100
rift_injected_latency_seconds_bucket{le="0.5"} 250
rift_injected_latency_seconds_bucket{le="1"} 300
```

### Imposter Metrics (Mountebank Mode)

```prometheus
# Imposters count
rift_imposters_total 5

# Stubs per imposter
rift_stubs_total{port="4545"} 10
rift_stubs_total{port="4546"} 25

# Requests per imposter
rift_imposter_requests_total{port="4545", matched="true"} 500
rift_imposter_requests_total{port="4545", matched="false"} 12
```

### Script Execution Metrics

```prometheus
# Script execution time
rift_script_execution_seconds_bucket{engine="rhai", le="0.001"} 950
rift_script_execution_seconds_bucket{engine="rhai", le="0.01"} 999
rift_script_execution_seconds_sum{engine="rhai"} 2.5
rift_script_execution_seconds_count{engine="rhai"} 1000

# Script errors
rift_script_errors_total{engine="rhai"} 5
```

### Flow State Metrics

```prometheus
# Flow state operations
rift_flow_state_operations_total{operation="get"} 5000
rift_flow_state_operations_total{operation="set"} 2000
rift_flow_state_operations_total{operation="delete"} 100

# Flow state operation latency
rift_flow_state_operation_seconds_bucket{operation="get", le="0.0001"} 4900
rift_flow_state_operation_seconds_bucket{operation="get", le="0.001"} 5000
```

### Connection Metrics

```prometheus
# Active connections
rift_active_connections 25

# Connection pool stats
rift_connection_pool_size{upstream="backend"} 10
rift_connection_pool_available{upstream="backend"} 7
```

---

## Prometheus Configuration

### Basic Scrape Config

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'rift'
    static_configs:
      - targets: ['localhost:9090']
    scrape_interval: 15s
```

### Kubernetes Service Discovery

```yaml
scrape_configs:
  - job_name: 'rift'
    kubernetes_sd_configs:
      - role: pod
    relabel_configs:
      - source_labels: [__meta_kubernetes_pod_label_app]
        regex: rift
        action: keep
      - source_labels: [__meta_kubernetes_pod_container_port_number]
        regex: "9090"
        action: keep
```

---

## Grafana Dashboard

### Import Dashboard

1. Go to Grafana → Dashboards → Import
2. Upload the dashboard JSON or paste the ID
3. Select your Prometheus data source

### Key Panels

**Request Rate:**
```promql
rate(rift_http_requests_total[5m])
```

**Error Rate:**
```promql
sum(rate(rift_http_requests_total{status=~"5.."}[5m]))
/
sum(rate(rift_http_requests_total[5m])) * 100
```

**P99 Latency:**
```promql
histogram_quantile(0.99, rate(rift_http_request_duration_seconds_bucket[5m]))
```

**Fault Injection Rate:**
```promql
sum(rate(rift_faults_injected_total[5m])) by (type)
```

### Sample Dashboard JSON

```json
{
  "title": "Rift Metrics",
  "panels": [
    {
      "title": "Request Rate",
      "type": "graph",
      "targets": [{
        "expr": "sum(rate(rift_http_requests_total[5m])) by (status)"
      }]
    },
    {
      "title": "Latency Percentiles",
      "type": "graph",
      "targets": [
        { "expr": "histogram_quantile(0.50, rate(rift_http_request_duration_seconds_bucket[5m]))", "legendFormat": "p50" },
        { "expr": "histogram_quantile(0.95, rate(rift_http_request_duration_seconds_bucket[5m]))", "legendFormat": "p95" },
        { "expr": "histogram_quantile(0.99, rate(rift_http_request_duration_seconds_bucket[5m]))", "legendFormat": "p99" }
      ]
    },
    {
      "title": "Fault Injection",
      "type": "graph",
      "targets": [{
        "expr": "sum(rate(rift_faults_injected_total[5m])) by (type)"
      }]
    }
  ]
}
```

---

## Alerting Rules

### High Error Rate

```yaml
groups:
  - name: rift
    rules:
      - alert: RiftHighErrorRate
        expr: |
          sum(rate(rift_http_requests_total{status=~"5.."}[5m]))
          /
          sum(rate(rift_http_requests_total[5m])) > 0.05
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High error rate in Rift"
          description: "Error rate is {{ $value | humanizePercentage }}"
```

### High Latency

```yaml
      - alert: RiftHighLatency
        expr: |
          histogram_quantile(0.99, rate(rift_http_request_duration_seconds_bucket[5m])) > 1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High latency in Rift"
          description: "P99 latency is {{ $value | humanizeDuration }}"
```

### Script Errors

```yaml
      - alert: RiftScriptErrors
        expr: rate(rift_script_errors_total[5m]) > 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Script errors in Rift"
          description: "Script engine {{ $labels.engine }} has errors"
```

---

## Best Practices

1. **Set reasonable scrape intervals** - 15-30 seconds is typical
2. **Use recording rules** - Pre-compute expensive queries
3. **Set up alerting** - Monitor error rates and latency
4. **Retain metrics** - Keep enough history for analysis
5. **Label cardinality** - Avoid high-cardinality labels (like request IDs)
