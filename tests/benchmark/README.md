# Rift vs Mountebank Performance Benchmark

This benchmark suite compares the performance of Rift (Rust-based HTTP proxy) against Mountebank (Node.js-based service virtualization tool) using identical configurations.

## Quick Start

```bash
# 1. Install required tools
./scripts/install-tools.sh

# 2. Start the benchmark environment
docker compose up -d --build

# 3. Run the benchmark suite
./scripts/run-benchmark.sh

# 4. View results
cat results/BENCHMARK_REPORT.md

# 5. Cleanup
docker compose down -v
```

## Direct-process mode (no Docker)

`scripts/bench_direct.py` runs the same comparison without Docker — useful on
machines where cgroup limits aren't available (e.g. macOS) or where you'd
rather give each engine the whole box.

```bash
# Prereqs: oha (load generator) on PATH, a release Rift binary, and mountebank
#   cargo build --release -p rift-http-proxy
#   npm install mountebank@2.9.1        # into ~/bench-mb, or pass --mb-bin

python3 scripts/bench_direct.py --run-all \
    --duration 20s --warmup 3s --connections 50 \
    --rift-bin ../../target/release/rift-http-proxy \
    --mb-bin ~/bench-mb/node_modules/mountebank/bin/mb

cat results/DIRECT_BENCHMARK_REPORT.md
```

How it stays fair and correct:

- **Sequential, not concurrent** — each engine runs alone, so they never
  contend for CPU on a shared machine.
- **Disjoint port ranges** — Rift on `2525`/`4545+`, Mountebank on
  `2625`/`4645+`. Even if one engine fails to shut down it cannot be measured
  in place of the other.
- **Hard teardown** — each engine is launched in its own process group and
  killed by group + `lsof`; its ports must be confirmed free before the next
  engine starts.
- **Response assertions** — every scenario sends one real request first and
  checks the returned **body** (not just a 2xx status) proves the intended stub
  matched. A request that falls through to the empty no-match default aborts the
  run, so a mis-configured stub can't silently inflate throughput.
- **Identical configs** — both engines get byte-identical imposter JSON.

> `oha` initialises a TLS stack that reads the macOS keychain even for
> plain-HTTP targets, so run this outside a restricted sandbox.


Install all tools automatically:
```bash
./scripts/install-tools.sh
```

## Admin create/read mode

`scripts/bench_direct.py` measures request *serving*. `scripts/bench_admin.py` measures the other
side of the engine — the **admin control plane**: the cost of creating an imposter with many stubs
and reading it back. This is where Rift's stub-overlap analysis lives (issue #423), a Rift
extension Mountebank does not perform, so it is where the two engines' admin behaviour differs
most.

```bash
python3 scripts/bench_admin.py --run-all \
    --rift-bin ../../target/release/rift-http-proxy \
    --mb-bin ~/bench-mb/node_modules/mountebank/bin/mb

cat results/ADMIN_BENCHMARK_REPORT.md
```

For each engine and each (predicate shape, stub count) it launches a **fresh** engine process (so
the RSS delta is isolated), `POST`s one imposter, then `GET`s it five times, recording create
latency, GET latency, process RSS delta, response size, and Rift's `_rift.warnings` count
(Mountebank's is always 0). Two shapes are exercised: `identical/overlap` (all stubs share one
predicate — the O(n²)-prone case #423 fixed) and `distinct` (the cheap control). Same
sequential/disjoint-port/hard-teardown discipline as `bench_direct.py`.



### Test Environment

Both services run with identical resource constraints:
- **CPUs:** 2 cores
- **Memory:** 1GB RAM
- **Network:** Docker bridge network

### Imposters Configuration

The benchmark creates 12 imposters with varying complexity:

| Port (MB/Rift) | Name | Stubs | Description |
|----------------|------|-------|-------------|
| 4545/5545 | API Server | ~500 | REST API simulation with CRUD endpoints |
| 4546/5546 | Regex Matcher | 100 | Regex pattern matching stubs |
| 4547/5547 | Complex Predicates | 50 | AND/OR predicate combinations |
| 4548/5548 | Behaviors | 20 | Wait/delay behaviors |
| 4549/5549 | Simple Baseline | 2 | Minimal stubs for baseline |
| 4550/5550 | JSON Body Matcher | 100 | JSON body equals/contains predicates |
| 4551/5551 | JSONPath Matcher | 100 | JSONPath expression predicates |
| 4552/5552 | XPath Matcher | 100 | XPath expression predicates (XML) |
| 4553/5553 | Template Responses | 50 | EJS template response generation |
| 4554/5554 | Header Router | 100 | Header-based routing predicates |
| 4555/5555 | Query Param Matcher | 100 | Query string matching predicates |
| 4556/5556 | Decorate Behaviors | 20 | JavaScript injection behaviors |

**Total: ~1140+ stubs across all imposters**

### Test Scenarios

1. **Simple Baseline** - Health check endpoints with minimal stubs
2. **Admin API** - Imposter listing and retrieval operations
3. **API Endpoints** - REST API with many stubs (first/middle/last match)
4. **Regex Matching** - Pattern matching with regex predicates
5. **Complex Predicates** - AND/OR/NOT predicate combinations
6. **JSON Body Matching** - Body equals and contains predicates
7. **JSONPath Predicates** - JSONPath expression matching
8. **XPath Predicates** - XPath expression matching for XML
9. **Template Responses** - EJS template rendering with variables
10. **Header Routing** - Header-based request routing
11. **Query Parameter Matching** - Query string predicate matching
12. **Decorate Behaviors** - JavaScript injection for response modification
13. **Stress Test** - High concurrency (200 connections)

## Running Benchmarks

### Full Benchmark Suite

```bash
# Default: 30s duration, 50 concurrent connections
./scripts/run-benchmark.sh
```

### Custom Configuration

```bash
# Longer duration, more connections
DURATION=60s CONNECTIONS=100 ./scripts/run-benchmark.sh

# Quick smoke test
DURATION=10s CONNECTIONS=20 ./scripts/run-benchmark.sh
```

### Manual Testing

```bash
# Setup imposters only
./scripts/setup-imposters.sh

# Test individual endpoints
hey -z 10s -c 50 http://localhost:4545/api/v1/resource1  # Mountebank
hey -z 10s -c 50 http://localhost:5545/api/v1/resource1  # Rift
```

## Results

Results are saved in the `results/` directory:

- `BENCHMARK_REPORT.md` - Docker-suite summary report with tables (tracked)
- `results.csv` - Docker-suite raw data in CSV format (tracked)
- `mountebank_detailed.txt` / `rift_detailed.txt` - Full `hey` output per engine (gitignored)
- `DIRECT_BENCHMARK_REPORT.md` / `direct_*.csv` - `bench_direct.py` native run (gitignored, machine-specific)
- `ADMIN_BENCHMARK_REPORT.md` - `bench_admin.py` admin create/read run (gitignored, machine-specific)

### Interpreting Results

- **RPS (Requests/sec)** - Higher is better
- **Latency** - Lower is better (measured in ms or seconds)
- **P50/P99** - 50th and 99th percentile latencies
- **Improvement** - Percentage improvement of Rift over Mountebank

## Benchmark Findings

### Latest Results (2026-07-09)

Regenerated from the Docker suite (`docker compose up -d --build` + `scripts/run-benchmark.sh`)
so the numbers are directly comparable to the previous run.

- **Rift:** `0.1.0` @ `e539853` · **Mountebank:** `2.9.2`
- **Config:** 15s per scenario, 50 concurrent connections, both engines capped at **2 CPUs / 1GB RAM**
- **Host:** Apple M4 (10 cores), macOS · `hey` load generator
- **Fixture:** 12 imposters, ~1140 stubs (see the table above)

> These are the resource-constrained Docker numbers. Run unconstrained on native
> processes (`scripts/bench_direct.py`, see [Direct-process mode](#direct-process-mode-no-docker)),
> Rift serves **160k–205k RPS** on the same host; those numbers aren't comparable to the
> capped run below and are regenerated per machine.

#### Core Functionality

| Test Scenario | Mountebank (RPS) | Rift (RPS) | Speedup | Avg Latency (MB → Rift) |
|---------------|------------------|------------|---------|-------------------------|
| Simple: Health Check | 2,308 | 44,901 | **19x faster** | 21.7ms → 1.1ms |
| Simple: Ping/Pong | 2,051 | 54,994 | **27x faster** | 24.4ms → 0.9ms |
| Admin: List Imposters | 9,884 | 29,747 | **3.0x faster** | 5.1ms → 1.7ms |
| Admin: Get Imposter | 452 | 758 | **1.7x faster** | 110.3ms → 65.8ms |

#### API Stub Matching (500 stubs)

| Test Scenario | Mountebank (RPS) | Rift (RPS) | Speedup | Avg Latency (MB → Rift) |
|---------------|------------------|------------|---------|-------------------------|
| API: First Stub Match | 2,342 | 40,226 | **17x faster** | 21.3ms → 1.2ms |
| API: Middle Stub Match | 590 | 41,978 | **71x faster** | 84.3ms → 1.2ms |
| API: Last Stub Match | 364 | 37,905 | **104x faster** | 136.8ms → 1.3ms |
| API: No Match (404) | 261 | 42,296 | **162x faster** | 190.6ms → 1.2ms |

#### JSON Body Matching

| Test Scenario | Mountebank (RPS) | Rift (RPS) | Speedup | Avg Latency (MB → Rift) |
|---------------|------------------|------------|---------|-------------------------|
| JSON: Body Equals (First) | 2,301 | 45,417 | **20x faster** | 21.7ms → 1.1ms |
| JSON: Body Equals (Middle) | 1,662 | 45,052 | **27x faster** | 30.1ms → 1.1ms |
| JSON: Body Contains | 2,078 | 47,831 | **23x faster** | 24.1ms → 1.0ms |

#### JSONPath Predicates (Standout Performance)

| Test Scenario | Mountebank (RPS) | Rift (RPS) | Speedup | Avg Latency (MB → Rift) |
|---------------|------------------|------------|---------|-------------------------|
| JSONPath: First Match | 136 | 42,043 | **310x faster** | 363.7ms → 1.2ms |
| JSONPath: Middle Match | 177 | 33,065 | **187x faster** | 280.1ms → 1.5ms |
| JSONPath: Last Match | 185 | 29,402 | **159x faster** | 268.4ms → 1.7ms |

#### XPath Predicates

| Test Scenario | Mountebank (RPS) | Rift (RPS) | Speedup | Avg Latency (MB → Rift) |
|---------------|------------------|------------|---------|-------------------------|
| XPath: First Match | 240 | 39,791 | **166x faster** | 206.5ms → 1.3ms |
| XPath: Middle Match | 236 | 8,964 | **38x faster** | 210.1ms → 5.6ms |
| XPath: Last Match | 82 | 3,168 | **39x faster** | 598.4ms → 15.8ms |

> XPath is the one matcher where **Rift itself degrades with stub position** (40k → 9k → 3k):
> an XPath selector can't be hash-dispatched, so each candidate stub re-evaluates the document.
> It's still 38–166x faster than Mountebank, but it's Rift's weakest predicate and the clearest
> optimization target.

#### Regex Matching

| Test Scenario | Mountebank (RPS) | Rift (RPS) | Speedup | Avg Latency (MB → Rift) |
|---------------|------------------|------------|---------|-------------------------|
| Regex: First Pattern | 2,049 | 47,440 | **23x faster** | 24.4ms → 1.1ms |
| Regex: Middle Pattern | 174 | 40,800 | **235x faster** | 285.3ms → 1.2ms |
| Regex: Last Pattern | 94 | 31,234 | **331x faster** | 522.3ms → 1.6ms |

> Regex is the biggest mover since the previous run: Rift went from ~130–7,000 RPS (and heavy
> position-dependent decay) to a flat **31k–47k RPS**. It no longer collapses on the 100th pattern.

#### Template Responses

| Test Scenario | Mountebank (RPS) | Rift (RPS) | Speedup | Avg Latency (MB → Rift) |
|---------------|------------------|------------|---------|-------------------------|
| Template: Simple | 1,705 | 19,454 | **11x faster** | 29.3ms → 2.6ms |
| Template: With Query | 1,684 | 43,071 | **26x faster** | 29.7ms → 1.2ms |

#### Header & Query Routing

| Test Scenario | Mountebank (RPS) | Rift (RPS) | Speedup | Avg Latency (MB → Rift) |
|---------------|------------------|------------|---------|-------------------------|
| Header: First Route | 2,258 | 43,801 | **19x faster** | 22.1ms → 1.1ms |
| Header: Middle Route | 1,447 | 26,149 | **18x faster** | 34.5ms → 1.9ms |
| Header: Last Route | 425 | 18,284 | **43x faster** | 117.2ms → 2.7ms |
| Query: First Match | 1,998 | 37,741 | **19x faster** | 25.0ms → 1.3ms |
| Query: Middle Match | 1,496 | 32,211 | **22x faster** | 33.4ms → 1.6ms |
| Query: Last Match | 968 | 23,554 | **24x faster** | 51.6ms → 2.1ms |

#### Decorate Behaviors & Stress

| Test Scenario | Mountebank (RPS) | Rift (RPS) | Speedup | Avg Latency (MB → Rift) |
|---------------|------------------|------------|---------|-------------------------|
| Decorate: First | 2,183 | 12,735 | **5.8x faster** | 22.9ms → 3.9ms |
| Decorate: Middle | 1,719 | 12,478 | **7.3x faster** | 29.1ms → 4.0ms |
| Complex: AND/OR | 957 | 40,842 | **43x faster** | 52.1ms → 1.2ms |
| Stress: 200 Concurrent | 2,276 | 49,918 | **22x faster** | 87.8ms → 4.0ms |

### Key Findings

1. **Regex, massively improved**: The biggest change since the previous run. Rift's regex path went
   from ~130–7,000 RPS (with steep position-dependent decay) to a flat **31k–47k RPS**, no longer
   collapsing at the 100th pattern. Now **23–331x** faster than Mountebank, whose JS `RegExp` scan
   falls to 94 RPS at the last pattern.

2. **Stub-position independence**: For hash-dispatched predicates (exact path/method, JSON body,
   header/query, regex) Rift holds **~30k–55k RPS** whether the matching stub is first or last, and
   on a no-match 404. Mountebank degrades linearly with stub count (2,342 → 261 RPS,
   first → no-match) — up to **162x** at the tail.

3. **JSONPath / complex predicates**: **43–310x** faster. Native Rust evaluation stays 29k–43k RPS
   while Mountebank's JavaScript path runs 136–960 RPS.

4. **XPath is the exception**: XPath is the only matcher where *Rift* degrades with stub position
   (39,791 → 8,964 → 3,168 RPS) because an XPath selector can't be hash-dispatched. Still 38–166x
   faster than Mountebank, but it's Rift's weakest predicate and the clearest thing left to optimize.

5. **Latency**: Rift average latency stays **0.9–2.7ms** on hash-dispatched scenarios (XPath-last and
   decorate are the outliers at 15.8ms / 3.9ms), while Mountebank ranges 5–598ms depending on stub
   count, match position, and predicate type.

6. **Templates & decorate**: The smallest margins — templates **11–26x**, decorate (JS injection)
   **5.8–7.3x** — because both engines execute JavaScript on that path; Rift wins on request-handling
   overhead rather than the interpreter.

7. **High concurrency**: Under 200 concurrent connections Rift sustains 49,918 RPS vs Mountebank's
   2,276 RPS (**22x**).

8. **Admin plane / overlap analysis** (`scripts/bench_admin.py`): creating 1,000 fully-overlapping
   stubs — the O(n²) case issue #423 fixed — Rift creates in **5.0ms vs Mountebank's 60ms** and grows
   RSS **+9MB vs +72MB (8x less memory)**, while still computing 101 stub-overlap warnings that
   Mountebank does not produce.

### Architecture Comparison

| Aspect | Mountebank | Rift |
|--------|------------|------|
| Language | Node.js (JavaScript) | Rust |
| Concurrency | Single-threaded event loop | Multi-threaded (Tokio) |
| Memory Model | Garbage collected | Zero-copy, no GC |
| Regex Engine | JavaScript RegExp | Rust regex crate |
| Stub Matching | Linear scan | Optimized matching |

### When to Choose Rift

Rift is recommended when you need:
- **High throughput**: 10-100x more requests per second
- **Low latency**: Sub-millisecond response times
- **Many stubs**: Performance doesn't degrade with stub count
- **High concurrency**: Efficient handling of many connections
- **Resource efficiency**: Lower CPU and memory usage

## Troubleshooting

### Services Not Starting

```bash
# Check container logs
docker logs mb-bench
docker logs rift-bench

# Verify ports are available
lsof -i :2525
lsof -i :3525
```

### hey Not Found

```bash
# macOS
brew install hey

# Linux (with Go installed)
go install github.com/rakyll/hey@latest

# Linux (direct download)
sudo curl -sSL https://hey-release.s3.us-east-2.amazonaws.com/hey_linux_amd64 -o /usr/local/bin/hey
sudo chmod +x /usr/local/bin/hey
```

### Connection Refused Errors

Wait for services to be healthy:
```bash
# Check health status
docker inspect --format='{{.State.Health.Status}}' mb-bench
docker inspect --format='{{.State.Health.Status}}' rift-bench
```

## Architecture Notes

### Why These Tests?

The benchmark suite is designed to test real-world scenarios:

1. **API Server (500 stubs):** Simulates a microservice with multiple REST endpoints, testing stub lookup performance with a large stub count.

2. **Regex Matching:** Tests the regex engine performance, which is critical for path matching and request body validation.

3. **Complex Predicates:** Tests the predicate evaluation engine with nested AND/OR logic.

4. **High Concurrency:** Tests how well each service handles many simultaneous connections.

### Fair Comparison Methodology

- Both services run in containers with identical resource limits
- Same imposter configurations are loaded via the Mountebank API
- Tests run sequentially to avoid resource contention
- Multiple requests ensure warm caches and JIT compilation
- Results include both raw numbers and percentage comparisons

## Contributing

To add new benchmark scenarios:

1. Add stub generation in `scripts/setup-imposters.sh`
2. Add benchmark function in `scripts/run-benchmark.sh`
3. Document the scenario in this README

## Related

- [Compatibility Tests](../compatibility/) - Functional compatibility tests
- [Integration Tests](../integration/) - Integration test suite
- [Mountebank Documentation](http://www.mbtest.org/)
