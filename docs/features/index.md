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

### Service Virtualization (Mountebank Mode)

- **Imposters** - Mock HTTP/HTTPS servers
- **Predicates** - Flexible request matching
- **Responses** - Static, proxy, and dynamic responses
- **Behaviors** - Response modification and delays

### Chaos Engineering (Native Mode)

- **Fault Injection** - Probabilistic latency and error injection
- **Scripting** - Rhai, Lua, and JavaScript engines
- **Flow State** - Stateful scenarios across requests
- **Metrics** - Prometheus integration

---

## Feature Comparison

| Feature | Mountebank Mode | Native Mode |
|:--------|:----------------|:------------|
| HTTP/HTTPS Mocking | Yes | Yes |
| Request Matching | Full predicates | Path/header matching |
| Static Responses | Yes | Via proxy |
| Proxy Recording | Yes | No |
| JavaScript Injection | Yes | Yes |
| Probabilistic Faults | Via injection | Built-in |
| Rhai/Lua Scripting | No | Yes |
| Flow State | Via injection | Built-in |
| Prometheus Metrics | Yes | Yes |
| Multi-upstream Routing | No | Yes |

---

## Feature Documentation

- [Fault Injection]({{ site.baseurl }}/features/fault-injection/) - Latency and error simulation
- [Scripting]({{ site.baseurl }}/features/scripting/) - Dynamic behavior with scripts
- [TLS/HTTPS]({{ site.baseurl }}/features/tls/) - Secure connections
- [Metrics]({{ site.baseurl }}/features/metrics/) - Prometheus monitoring
