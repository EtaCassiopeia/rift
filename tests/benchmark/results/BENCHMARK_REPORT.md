# Rift vs Mountebank Benchmark Report

**Date:** 2026-07-09 22:27:26
**Duration per test:** 15s
**Concurrent connections:** 50

## Summary Results

| Test | MB RPS | Rift RPS | Speedup |
|------|--------|----------|---------|
| Simple: Health Check | 2307.5350 | 44901.3402 | **19.4x faster** |
| Simple: Ping/Pong | 2051.0898 | 54993.6532 | **26.8x faster** |
| Admin: List Imposters | 9884.2184 | 29747.0758 | **3.0x faster** |
| Admin: Get Imposter | 451.5615 | 758.4748 | **1.6x faster** |
| API: First Stub Match | 2341.7705 | 40226.3164 | **17.1x faster** |
| API: Middle Stub Match | 590.1220 | 41978.3325 | **71.1x faster** |
| API: Last Stub Match | 364.0382 | 37904.7336 | **104.1x faster** |
| API: No Match (404) | 260.9594 | 42296.2581 | **162.0x faster** |
| Regex: First Pattern | 2049.0658 | 47439.9753 | **23.1x faster** |
| Regex: Middle Pattern | 173.7418 | 40800.2344 | **234.8x faster** |
| Regex: Last Pattern | 94.2081 | 31233.5833 | **331.5x faster** |
| Complex: AND/OR Predicates | 957.1441 | 40842.0531 | **42.6x faster** |
| JSON: Body Equals (First) | 2300.8824 | 45417.3607 | **19.7x faster** |
| JSON: Body Equals (Middle) | 1661.5914 | 45051.6827 | **27.1x faster** |
| JSON: Body Contains | 2077.6748 | 47831.2308 | **23.0x faster** |
| JSONPath: First Match | 135.8246 | 42042.8192 | **309.5x faster** |
| JSONPath: Middle Match | 177.0318 | 33065.4261 | **186.7x faster** |
| JSONPath: Last Match | 184.7958 | 29402.0258 | **159.1x faster** |
| XPath: First Match | 240.1051 | 39790.7662 | **165.7x faster** |
| XPath: Middle Match | 236.0032 | 8964.0802 | **37.9x faster** |
| XPath: Last Match | 81.9581 | 3167.7419 | **38.6x faster** |
| Template: Simple | 1705.0659 | 19453.7320 | **11.4x faster** |
| Template: With Query | 1684.2506 | 43071.3256 | **25.5x faster** |
| Decorate: First | 2182.5096 | 12734.8190 | **5.8x faster** |
| Decorate: Middle | 1719.4875 | 12477.6042 | **7.2x faster** |
| Header: First Route | 2257.8490 | 43800.6331 | **19.3x faster** |
| Header: Middle Route | 1446.6805 | 26148.8714 | **18.0x faster** |
| Header: Last Route | 424.7963 | 18284.0564 | **43.0x faster** |
| Query: First Match | 1998.3611 | 37741.4300 | **18.8x faster** |
| Query: Middle Match | 1495.9094 | 32211.0560 | **21.5x faster** |
| Query: Last Match | 967.6903 | 23554.0789 | **24.3x faster** |
| Stress: 200 Concurrent | 2276.1667 | 49917.6269 | **21.9x faster** |

## Latency Comparison (P99)

| Test | MB P99 | Rift P99 |
|------|--------|----------|
| Simple: Health Check | 0.0705 | 0.0024 |
| Simple: Ping/Pong | 0.0767 | 0.0017 |
| Admin: List Imposters | 0.0093 | 0.0039 |
| Admin: Get Imposter | 0.2016 | 0.1100 |
| API: First Stub Match | 0.0558 | 0.0038 |
| API: Middle Stub Match | 0.2231 | 0.0025 |
| API: Last Stub Match | 0.1850 | 0.0030 |
| API: No Match (404) | 0.2660 | 0.0023 |
| Regex: First Pattern | 0.0511 | 0.0020 |
| Regex: Middle Pattern | 0.3518 | 0.0028 |
| Regex: Last Pattern | 0.5778 | 0.0035 |
| Complex: AND/OR Predicates | 0.0913 | 0.0026 |
| JSON: Body Equals (First) | 0.0585 | 0.0022 |
| JSON: Body Equals (Middle) | 0.0602 | 0.0021 |
| JSON: Body Contains | 0.0584 | 0.0019 |
| JSONPath: First Match | 0.7365 | 0.0025 |
| JSONPath: Middle Match | 0.5228 | 0.0037 |
| JSONPath: Last Match | 0.4996 | 0.0043 |
| XPath: First Match | 0.3209 | 0.0026 |
| XPath: Middle Match | 0.2686 | 0.0151 |
| XPath: Last Match | 0.7654 | 0.0585 |
| Template: Simple | 0.1021 | 0.0137 |
| Template: With Query | 0.0722 | 0.0022 |
| Decorate: First | 0.0670 | 0.0094 |
| Decorate: Middle | 0.0999 | 0.0075 |
| Header: First Route | 0.0630 | 0.0022 |
| Header: Middle Route | 0.0735 | 0.0084 |
| Header: Last Route | 0.2647 | 0.0115 |
| Query: First Match | 0.0770 | 0.0028 |
| Query: Middle Match | 0.0604 | 0.0038 |
| Query: Last Match | 0.0891 | 0.0046 |
| Stress: 200 Concurrent | 0.2110 | 0.0074 |

## Configuration

- **Imposters:** 12 (API Server, Regex, Complex Predicates, Behaviors, JSON Body, JSONPath, XPath, Templates, Header Routing, Query Params, Decorate, Simple Baseline)
- **Total Stubs:** ~1140+ stubs across all imposters
- **Resource Limits:** 2 CPUs, 1GB RAM per service
