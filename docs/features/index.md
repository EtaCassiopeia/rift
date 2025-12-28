---
layout: default
title: Features
nav_order: 5
has_children: true
permalink: /features/
---

# Features

Rift provides advanced features for service virtualization and chaos engineering.

---

## Core Features

### Mountebank Compatibility

- **Imposters** - Mock HTTP/HTTPS servers
- **Predicates** - Flexible request matching
- **Responses** - Static, proxy, and dynamic responses
- **Behaviors** - Response modification and delays
- **JavaScript Injection** - Dynamic response generation

### Rift Extensions (`_rift` Namespace)

- **Fault Injection** - Probabilistic latency, error, and TCP fault injection
- **Scripting** - Rhai, Lua, and JavaScript engines for dynamic behavior
- **Flow State** - Stateful scenarios with InMemory or Redis backends
- **Stub Analysis** - Overlap detection and conflict warnings
- **Debug Mode** - Request matching diagnostics with `X-Rift-Debug` header
- **Metrics** - Prometheus integration

---

## Feature Overview

| Feature | Mountebank | Rift Extensions |
|:--------|:-----------|:----------------|
| HTTP/HTTPS Mocking | ✅ Full support | — |
| Request Matching | ✅ Full predicates | — |
| Static Responses | ✅ | — |
| Proxy Recording | ✅ | — |
| JavaScript Injection | ✅ | — |
| Probabilistic Faults | Via injection | ✅ `_rift.fault` |
| Rhai/Lua Scripting | — | ✅ `_rift.script` |
| Flow State | Via injection | ✅ `_rift.flowState` |
| Stub Analysis | — | ✅ `_rift.warnings` |
| Stub IDs | — | ✅ `id` field |
| Debug Mode | — | ✅ `X-Rift-Debug` header |
| Prometheus Metrics | ✅ | ✅ |
| Config Linting | — | ✅ `rift-lint` |
| Terminal UI | — | ✅ `rift-tui` |

---

## Feature Documentation

- [Fault Injection]({{ site.baseurl }}/features/fault-injection/) - Latency and error simulation
- [Scripting]({{ site.baseurl }}/features/scripting/) - Dynamic behavior with scripts
- [Stub Analysis]({{ site.baseurl }}/features/stub-analysis/) - Overlap detection and warnings
- [Debug Mode]({{ site.baseurl }}/features/debug-mode/) - Request matching diagnostics
- [TLS/HTTPS]({{ site.baseurl }}/features/tls/) - Secure connections
- [Metrics]({{ site.baseurl }}/features/metrics/) - Prometheus monitoring
- [Configuration Linting]({{ site.baseurl }}/features/linting/) - Validate imposter configs before loading
- [Terminal UI]({{ site.baseurl }}/features/tui/) - Interactive imposter management
