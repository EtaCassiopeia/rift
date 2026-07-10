#!/bin/bash
# Benchmark runner for Rift vs Mountebank performance comparison
#
# This script runs a series of HTTP load tests against both services
# and generates a comparison report.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCHMARK_DIR="$(dirname "$SCRIPT_DIR")"
RESULTS_DIR="$BENCHMARK_DIR/results"

# Default configuration
DURATION="${DURATION:-30s}"
CONNECTIONS="${CONNECTIONS:-50}"
THREADS="${THREADS:-4}"
RATE="${RATE:-0}"  # 0 = unlimited for hey

# Service ports
MB_ADMIN_PORT=2525
RIFT_ADMIN_PORT=3525
MB_IMPOSTER_BASE=4545
RIFT_IMPOSTER_BASE=5545

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_section() { echo -e "\n${BLUE}=== $1 ===${NC}\n"; }
log_result() { echo -e "${CYAN}[RESULT]${NC} $1"; }

# Check for required tools
check_tools() {
    local missing=()

    if ! command -v hey &> /dev/null; then
        missing+=("hey")
    fi

    if ! command -v curl &> /dev/null; then
        missing+=("curl")
    fi

    if ! command -v jq &> /dev/null; then
        missing+=("jq")
    fi

    if [ ${#missing[@]} -ne 0 ]; then
        echo -e "${RED}[ERROR]${NC} Missing required tools: ${missing[*]}"
        echo "Run: ./scripts/install-tools.sh"
        exit 1
    fi
}

# Wait for services to be ready
wait_for_services() {
    log_info "Waiting for services to be ready..."

    for i in {1..30}; do
        mb_ready=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$MB_ADMIN_PORT/" || echo "000")
        rift_ready=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$RIFT_ADMIN_PORT/" || echo "000")

        if [ "$mb_ready" = "200" ] && [ "$rift_ready" = "200" ]; then
            log_info "Both services are ready!"
            return 0
        fi

        echo "Waiting... (Mountebank: $mb_ready, Rift: $rift_ready)"
        sleep 2
    done

    echo -e "${RED}[ERROR]${NC} Services did not become ready in time"
    exit 1
}

# Run a single benchmark test
run_benchmark() {
    local name=$1
    local mb_url=$2
    local rift_url=$3
    local method=${4:-GET}
    local body=${5:-}
    local header_name=${6:-}
    local header_value=${7:-}

    log_section "Benchmark: $name"

    # Run Mountebank benchmark
    echo "Running against Mountebank..."
    local mb_result
    if [ -n "$body" ] && [ -n "$header_name" ]; then
        mb_result=$(hey -z "$DURATION" -c "$CONNECTIONS" -m "$method" -d "$body" -H "$header_name: $header_value" "$mb_url" 2>&1)
    elif [ -n "$body" ]; then
        mb_result=$(hey -z "$DURATION" -c "$CONNECTIONS" -m "$method" -d "$body" "$mb_url" 2>&1)
    elif [ -n "$header_name" ]; then
        mb_result=$(hey -z "$DURATION" -c "$CONNECTIONS" -m "$method" -H "$header_name: $header_value" "$mb_url" 2>&1)
    else
        mb_result=$(hey -z "$DURATION" -c "$CONNECTIONS" -m "$method" "$mb_url" 2>&1)
    fi

    # Run Rift benchmark
    echo "Running against Rift..."
    local rift_result
    if [ -n "$body" ] && [ -n "$header_name" ]; then
        rift_result=$(hey -z "$DURATION" -c "$CONNECTIONS" -m "$method" -d "$body" -H "$header_name: $header_value" "$rift_url" 2>&1)
    elif [ -n "$body" ]; then
        rift_result=$(hey -z "$DURATION" -c "$CONNECTIONS" -m "$method" -d "$body" "$rift_url" 2>&1)
    elif [ -n "$header_name" ]; then
        rift_result=$(hey -z "$DURATION" -c "$CONNECTIONS" -m "$method" -H "$header_name: $header_value" "$rift_url" 2>&1)
    else
        rift_result=$(hey -z "$DURATION" -c "$CONNECTIONS" -m "$method" "$rift_url" 2>&1)
    fi

    # Extract metrics
    mb_rps=$(echo "$mb_result" | grep "Requests/sec:" | awk '{print $2}')
    mb_avg=$(echo "$mb_result" | grep "Average:" | head -1 | awk '{print $2}')
    # hey prints "  50% in 0.0011 secs" — the latency value is field 3, not field 2.
    mb_p50=$(echo "$mb_result" | grep -E "^\s*50%" | awk '{print $3}' | head -1)
    mb_p99=$(echo "$mb_result" | grep -E "^\s*99%" | awk '{print $3}' | head -1)

    rift_rps=$(echo "$rift_result" | grep "Requests/sec:" | awk '{print $2}')
    rift_avg=$(echo "$rift_result" | grep "Average:" | head -1 | awk '{print $2}')
    rift_p50=$(echo "$rift_result" | grep -E "^\s*50%" | awk '{print $3}' | head -1)
    rift_p99=$(echo "$rift_result" | grep -E "^\s*99%" | awk '{print $3}' | head -1)

    # Default to N/A if not found
    [ -z "$mb_p50" ] && mb_p50="N/A"
    [ -z "$mb_p99" ] && mb_p99="N/A"
    [ -z "$rift_p50" ] && rift_p50="N/A"
    [ -z "$rift_p99" ] && rift_p99="N/A"

    # Calculate improvement as multiplier (Nx faster)
    if [ -n "$mb_rps" ] && [ -n "$rift_rps" ]; then
        improvement=$(echo "scale=1; $rift_rps / $mb_rps" | bc 2>/dev/null || echo "N/A")
    else
        improvement="N/A"
    fi

    # Save results
    echo "$name,$mb_rps,$mb_avg,$mb_p50,$mb_p99,$rift_rps,$rift_avg,$rift_p50,$rift_p99,$improvement" >> "$RESULTS_DIR/results.csv"

    # Display results
    echo ""
    printf "%-20s %15s %15s\n" "Metric" "Mountebank" "Rift"
    printf "%-20s %15s %15s\n" "--------------------" "---------------" "---------------"
    printf "%-20s %15s %15s\n" "Requests/sec" "$mb_rps" "$rift_rps"
    printf "%-20s %15s %15s\n" "Avg latency" "$mb_avg" "$rift_avg"
    printf "%-20s %15s %15s\n" "P50 latency" "$mb_p50" "$rift_p50"
    printf "%-20s %15s %15s\n" "P99 latency" "$mb_p99" "$rift_p99"
    echo ""
    log_result "Rift is ${improvement}x faster"

    # Save detailed results
    echo "=== $name ===" >> "$RESULTS_DIR/mountebank_detailed.txt"
    echo "$mb_result" >> "$RESULTS_DIR/mountebank_detailed.txt"
    echo "" >> "$RESULTS_DIR/mountebank_detailed.txt"

    echo "=== $name ===" >> "$RESULTS_DIR/rift_detailed.txt"
    echo "$rift_result" >> "$RESULTS_DIR/rift_detailed.txt"
    echo "" >> "$RESULTS_DIR/rift_detailed.txt"
}

# Run admin API benchmarks
benchmark_admin_api() {
    log_section "Admin API Benchmarks"

    # GET imposters list
    run_benchmark "Admin: List Imposters" \
        "http://localhost:$MB_ADMIN_PORT/imposters" \
        "http://localhost:$RIFT_ADMIN_PORT/imposters"

    # GET single imposter
    run_benchmark "Admin: Get Imposter" \
        "http://localhost:$MB_ADMIN_PORT/imposters/4545" \
        "http://localhost:$RIFT_ADMIN_PORT/imposters/4545"
}

# Run simple endpoint benchmarks
benchmark_simple() {
    log_section "Simple Endpoint Benchmarks (Baseline)"

    local mb_port=$((MB_IMPOSTER_BASE + 4))  # 4549
    local rift_port=$((RIFT_IMPOSTER_BASE + 4))  # 5549

    run_benchmark "Simple: Health Check" \
        "http://localhost:$mb_port/health" \
        "http://localhost:$rift_port/health"

    run_benchmark "Simple: Ping/Pong" \
        "http://localhost:$mb_port/ping" \
        "http://localhost:$rift_port/ping"
}

# Run API endpoint benchmarks
benchmark_api() {
    log_section "API Endpoint Benchmarks (Many Stubs)"

    local mb_port=$MB_IMPOSTER_BASE  # 4545
    local rift_port=$RIFT_IMPOSTER_BASE  # 5545

    # First stub match
    run_benchmark "API: First Stub Match" \
        "http://localhost:$mb_port/api/v1/resource1" \
        "http://localhost:$rift_port/api/v1/resource1"

    # Middle stub match
    run_benchmark "API: Middle Stub Match" \
        "http://localhost:$mb_port/api/v1/resource5/5" \
        "http://localhost:$rift_port/api/v1/resource5/5"

    # Last stub match
    run_benchmark "API: Last Stub Match" \
        "http://localhost:$mb_port/api/v1/resource10/10" \
        "http://localhost:$rift_port/api/v1/resource10/10"

    # No match (404)
    run_benchmark "API: No Match (404)" \
        "http://localhost:$mb_port/nonexistent" \
        "http://localhost:$rift_port/nonexistent"
}

# Run regex benchmark
benchmark_regex() {
    log_section "Regex Matching Benchmarks"

    local mb_port=$((MB_IMPOSTER_BASE + 1))  # 4546
    local rift_port=$((RIFT_IMPOSTER_BASE + 1))  # 5546

    run_benchmark "Regex: First Pattern" \
        "http://localhost:$mb_port/regex/pattern1/abc123" \
        "http://localhost:$rift_port/regex/pattern1/abc123"

    run_benchmark "Regex: Middle Pattern" \
        "http://localhost:$mb_port/regex/pattern50/xyz789" \
        "http://localhost:$rift_port/regex/pattern50/xyz789"

    run_benchmark "Regex: Last Pattern" \
        "http://localhost:$mb_port/regex/pattern100/test" \
        "http://localhost:$rift_port/regex/pattern100/test"
}

# Run complex predicate benchmark
benchmark_complex() {
    log_section "Complex Predicate Benchmarks"

    local mb_port=$((MB_IMPOSTER_BASE + 2))  # 4547
    local rift_port=$((RIFT_IMPOSTER_BASE + 2))  # 5547

    run_benchmark "Complex: AND/OR Predicates" \
        "http://localhost:$mb_port/complex/25/test" \
        "http://localhost:$rift_port/complex/25/test" \
        "POST" \
        '{"name":"test"}'
}

# Run concurrent connection stress test
benchmark_stress() {
    log_section "Stress Test (High Concurrency)"

    local old_connections=$CONNECTIONS
    CONNECTIONS=200

    local mb_port=$MB_IMPOSTER_BASE
    local rift_port=$RIFT_IMPOSTER_BASE

    run_benchmark "Stress: 200 Concurrent" \
        "http://localhost:$mb_port/api/v1/resource1" \
        "http://localhost:$rift_port/api/v1/resource1"

    CONNECTIONS=$old_connections
}

# Run JSON body matching benchmarks
benchmark_json_body() {
    log_section "JSON Body Matching Benchmarks"

    local mb_port=$((MB_IMPOSTER_BASE + 5))  # 4550
    local rift_port=$((RIFT_IMPOSTER_BASE + 5))  # 5550

    run_benchmark "JSON: Body Equals (First)" \
        "http://localhost:$mb_port/json/equals/1" \
        "http://localhost:$rift_port/json/equals/1" \
        "POST" \
        '{"id": 1, "type": "request"}'

    run_benchmark "JSON: Body Equals (Middle)" \
        "http://localhost:$mb_port/json/equals/25" \
        "http://localhost:$rift_port/json/equals/25" \
        "POST" \
        '{"id": 25, "type": "request"}'

    run_benchmark "JSON: Body Contains" \
        "http://localhost:$mb_port/json/contains/10" \
        "http://localhost:$rift_port/json/contains/10" \
        "POST" \
        '{"name":"item10", "data": "extra"}'
}

# Run JSONPath predicate benchmarks
benchmark_jsonpath() {
    log_section "JSONPath Predicate Benchmarks"

    local mb_port=$((MB_IMPOSTER_BASE + 6))  # 4551
    local rift_port=$((RIFT_IMPOSTER_BASE + 6))  # 5551

    run_benchmark "JSONPath: First Match" \
        "http://localhost:$mb_port/jsonpath/1" \
        "http://localhost:$rift_port/jsonpath/1" \
        "POST" \
        '{"user": {"id": 1, "name": "test"}}'

    run_benchmark "JSONPath: Middle Match" \
        "http://localhost:$mb_port/jsonpath/25" \
        "http://localhost:$rift_port/jsonpath/25" \
        "POST" \
        '{"user": {"id": 25, "name": "test"}}'

    run_benchmark "JSONPath: Last Match" \
        "http://localhost:$mb_port/jsonpath/50" \
        "http://localhost:$rift_port/jsonpath/50" \
        "POST" \
        '{"user": {"id": 50, "name": "test"}}'
}

# Run XPath predicate benchmarks
benchmark_xpath() {
    log_section "XPath Predicate Benchmarks"

    local mb_port=$((MB_IMPOSTER_BASE + 7))  # 4552
    local rift_port=$((RIFT_IMPOSTER_BASE + 7))  # 5552

    run_benchmark "XPath: First Match" \
        "http://localhost:$mb_port/xpath/1" \
        "http://localhost:$rift_port/xpath/1" \
        "POST" \
        '<root><item id="1">test</item></root>'

    run_benchmark "XPath: Middle Match" \
        "http://localhost:$mb_port/xpath/25" \
        "http://localhost:$rift_port/xpath/25" \
        "POST" \
        '<root><item id="25">test</item></root>'

    run_benchmark "XPath: Last Match" \
        "http://localhost:$mb_port/xpath/50" \
        "http://localhost:$rift_port/xpath/50" \
        "POST" \
        '<root><item id="50">test</item></root>'
}

# Run template response benchmarks
benchmark_templates() {
    log_section "Template Response Benchmarks"

    local mb_port=$((MB_IMPOSTER_BASE + 8))  # 4553
    local rift_port=$((RIFT_IMPOSTER_BASE + 8))  # 5553

    run_benchmark "Template: Simple" \
        "http://localhost:$mb_port/template/1" \
        "http://localhost:$rift_port/template/1"

    run_benchmark "Template: With Query" \
        "http://localhost:$mb_port/template/25?foo=bar&baz=qux" \
        "http://localhost:$rift_port/template/25?foo=bar&baz=qux"
}

# Run header-based routing benchmarks
benchmark_header_routing() {
    log_section "Header-Based Routing Benchmarks"

    local mb_port=$((MB_IMPOSTER_BASE + 9))  # 4554
    local rift_port=$((RIFT_IMPOSTER_BASE + 9))  # 5554

    run_benchmark "Header: First Route" \
        "http://localhost:$mb_port/headers/route" \
        "http://localhost:$rift_port/headers/route" \
        "GET" \
        "" \
        "X-Route-Id" \
        "route-1"

    run_benchmark "Header: Middle Route" \
        "http://localhost:$mb_port/headers/route" \
        "http://localhost:$rift_port/headers/route" \
        "GET" \
        "" \
        "X-Route-Id" \
        "route-50"

    run_benchmark "Header: Last Route" \
        "http://localhost:$mb_port/headers/route" \
        "http://localhost:$rift_port/headers/route" \
        "GET" \
        "" \
        "X-Route-Id" \
        "route-100"
}

# Run query parameter matching benchmarks
benchmark_query_params() {
    log_section "Query Parameter Matching Benchmarks"

    local mb_port=$((MB_IMPOSTER_BASE + 10))  # 4555
    local rift_port=$((RIFT_IMPOSTER_BASE + 10))  # 5555

    run_benchmark "Query: First Match" \
        "http://localhost:$mb_port/query/search?page=1&size=10" \
        "http://localhost:$rift_port/query/search?page=1&size=10"

    run_benchmark "Query: Middle Match" \
        "http://localhost:$mb_port/query/search?page=50&size=10" \
        "http://localhost:$rift_port/query/search?page=50&size=10"

    run_benchmark "Query: Last Match" \
        "http://localhost:$mb_port/query/search?page=100&size=10" \
        "http://localhost:$rift_port/query/search?page=100&size=10"
}

# Run decorate behavior benchmarks
benchmark_decorate() {
    log_section "Decorate Behavior Benchmarks"

    local mb_port=$((MB_IMPOSTER_BASE + 11))  # 4556
    local rift_port=$((RIFT_IMPOSTER_BASE + 11))  # 5556

    run_benchmark "Decorate: First" \
        "http://localhost:$mb_port/decorate/1" \
        "http://localhost:$rift_port/decorate/1"

    run_benchmark "Decorate: Middle" \
        "http://localhost:$mb_port/decorate/10" \
        "http://localhost:$rift_port/decorate/10"
}

# Generate summary report
generate_report() {
    log_section "Generating Summary Report"

    local report_file="$RESULTS_DIR/BENCHMARK_REPORT.md"

    cat > "$report_file" << 'EOF'
# Rift vs Mountebank Benchmark Report

EOF

    echo "**Date:** $(date '+%Y-%m-%d %H:%M:%S')" >> "$report_file"
    echo "**Duration per test:** $DURATION" >> "$report_file"
    echo "**Concurrent connections:** $CONNECTIONS" >> "$report_file"
    echo "" >> "$report_file"

    echo "## Summary Results" >> "$report_file"
    echo "" >> "$report_file"
    echo "| Test | MB RPS | Rift RPS | Speedup |" >> "$report_file"
    echo "|------|--------|----------|---------|" >> "$report_file"

    # Parse CSV and create table
    tail -n +2 "$RESULTS_DIR/results.csv" | while IFS=',' read -r name mb_rps mb_avg mb_p50 mb_p99 rift_rps rift_avg rift_p50 rift_p99 improvement; do
        printf "| %s | %s | %s | **%sx faster** |\n" "$name" "$mb_rps" "$rift_rps" "$improvement" >> "$report_file"
    done

    echo "" >> "$report_file"
    echo "## Latency Comparison (P99)" >> "$report_file"
    echo "" >> "$report_file"
    echo "| Test | MB P99 | Rift P99 |" >> "$report_file"
    echo "|------|--------|----------|" >> "$report_file"

    tail -n +2 "$RESULTS_DIR/results.csv" | while IFS=',' read -r name mb_rps mb_avg mb_p50 mb_p99 rift_rps rift_avg rift_p50 rift_p99 improvement; do
        printf "| %s | %s | %s |\n" "$name" "$mb_p99" "$rift_p99" >> "$report_file"
    done

    echo "" >> "$report_file"
    echo "## Configuration" >> "$report_file"
    echo "" >> "$report_file"
    echo "- **Imposters:** 12 (API Server, Regex, Complex Predicates, Behaviors, JSON Body, JSONPath, XPath, Templates, Header Routing, Query Params, Decorate, Simple Baseline)" >> "$report_file"
    echo "- **Total Stubs:** ~1140+ stubs across all imposters" >> "$report_file"
    echo "- **Resource Limits:** 2 CPUs, 1GB RAM per service" >> "$report_file"

    log_info "Report generated: $report_file"
}

# Main
main() {
    echo -e "${BLUE}"
    echo "╔═══════════════════════════════════════════════════════════╗"
    echo "║     Rift vs Mountebank Performance Benchmark Suite        ║"
    echo "╚═══════════════════════════════════════════════════════════╝"
    echo -e "${NC}"

    check_tools

    # Create results directory
    mkdir -p "$RESULTS_DIR"
    rm -f "$RESULTS_DIR/results.csv" "$RESULTS_DIR/mountebank_detailed.txt" "$RESULTS_DIR/rift_detailed.txt"

    # CSV header
    echo "Test,MB_RPS,MB_Avg,MB_P50,MB_P99,Rift_RPS,Rift_Avg,Rift_P50,Rift_P99,Speedup" > "$RESULTS_DIR/results.csv"

    wait_for_services

    # Setup imposters
    log_info "Setting up imposters..."
    "$SCRIPT_DIR/setup-imposters.sh"

    # Give services a moment to settle
    sleep 2

    # Run benchmarks - organized by category

    # Basic tests
    benchmark_simple
    benchmark_admin_api

    # Path and method matching
    benchmark_api
    benchmark_regex
    benchmark_complex

    # Body matching (JSON/XML)
    benchmark_json_body
    benchmark_jsonpath
    benchmark_xpath

    # Response features
    benchmark_templates
    benchmark_decorate

    # Routing tests
    benchmark_header_routing
    benchmark_query_params

    # Stress test
    benchmark_stress

    # Generate report
    generate_report

    log_section "Benchmark Complete!"
    echo "Results saved in: $RESULTS_DIR/"
    echo ""
    echo "Files:"
    ls -la "$RESULTS_DIR/"
}

main "$@"
