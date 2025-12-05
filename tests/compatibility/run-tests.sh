#!/bin/bash
#
# Side-by-side compatibility tests for Mountebank and Rift
#
# This script runs the same tests against both Mountebank and Rift
# and compares the results to ensure API compatibility.
#

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
MB_ADMIN="http://localhost:2525"
RIFT_ADMIN="http://localhost:3525"
MB_IMPOSTER_BASE="http://localhost"
RIFT_IMPOSTER_BASE="http://localhost"

# Port mapping (Mountebank -> Rift)
# Function to get Rift port from Mountebank port for compatibility
get_rift_port() {
    local mb_port="$1"
    case "$mb_port" in
        4545) echo 5545 ;;
        4546) echo 5546 ;;
        4547) echo 5547 ;;
        *)
            echo "Error: Unknown Mountebank port $mb_port" >&2
            exit 1
            ;;
    esac
}

# Test counters
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

# Temp files for comparison
MB_RESPONSE=$(mktemp)
RIFT_RESPONSE=$(mktemp)
trap "rm -f $MB_RESPONSE $RIFT_RESPONSE" EXIT

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[PASS]${NC} $1"
    ((TESTS_PASSED++)) || true
}

log_fail() {
    echo -e "${RED}[FAIL]${NC} $1"
    ((TESTS_FAILED++)) || true
}

log_skip() {
    echo -e "${YELLOW}[SKIP]${NC} $1"
    ((TESTS_SKIPPED++)) || true
}

log_section() {
    echo ""
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}  $1${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo ""
}

# Wait for services to be ready
wait_for_services() {
    log_info "Waiting for services to be ready..."

    for i in {1..30}; do
        if curl -s "$MB_ADMIN/" > /dev/null 2>&1 && \
           curl -s "$RIFT_ADMIN/" > /dev/null 2>&1; then
            log_info "Both services are ready"
            return 0
        fi
        sleep 1
    done

    echo "Services did not become ready in time"
    exit 1
}

# Compare HTTP status codes
compare_status() {
    local test_name="$1"
    local mb_status="$2"
    local rift_status="$3"

    if [ "$mb_status" == "$rift_status" ]; then
        log_success "$test_name - Status codes match ($mb_status)"
        return 0
    else
        log_fail "$test_name - Status codes differ (MB: $mb_status, Rift: $rift_status)"
        return 1
    fi
}

# Compare JSON responses (ignoring Rift-specific headers)
compare_json_response() {
    local test_name="$1"
    local mb_body="$2"
    local rift_body="$3"

    # Normalize JSON (sort keys, remove whitespace)
    local mb_normalized=$(echo "$mb_body" | jq -S '.' 2>/dev/null || echo "$mb_body")
    local rift_normalized=$(echo "$rift_body" | jq -S '.' 2>/dev/null || echo "$rift_body")

    if [ "$mb_normalized" == "$rift_normalized" ]; then
        log_success "$test_name - Response bodies match"
        return 0
    else
        log_fail "$test_name - Response bodies differ"
        echo "  Mountebank: $mb_body"
        echo "  Rift:       $rift_body"
        return 1
    fi
}

# Helper function to remap ports in JSON data for Rift
# NOTE: We do NOT remap ports in the JSON payload because Rift creates imposters
# on the same container port (4545). The Docker port mapping (host 5545 -> container 4545)
# handles the translation when accessing the imposter endpoints.
remap_ports_for_rift() {
    local data="$1"
    # No remapping needed - Rift uses the same container ports as Mountebank
    echo "$data"
}

# Helper function to remap ports in API path for Rift
# NOTE: We do NOT remap the API path because Rift's admin API references the container port (4545),
# not the host port (5545). The admin API path /imposters/4545 is correct for both systems.
remap_path_for_rift() {
    local path="$1"
    # No remapping needed - admin API uses container ports
    echo "$path"
}

# Generic API test
test_api() {
    local test_name="$1"
    local method="$2"
    local path="$3"
    local data="$4"
    local expected_status="$5"

    local mb_url="${MB_ADMIN}${path}"
    local rift_path=$(remap_path_for_rift "$path")
    local rift_url="${RIFT_ADMIN}${rift_path}"

    # Remap ports in data for Rift
    local rift_data=""
    if [ -n "$data" ]; then
        rift_data=$(remap_ports_for_rift "$data")
    fi

    if [ -n "$data" ]; then
        mb_result=$(curl -s -w $'\n%{http_code}' -X "$method" -H "Content-Type: application/json" -d "$data" "$mb_url")
        rift_result=$(curl -s -w $'\n%{http_code}' -X "$method" -H "Content-Type: application/json" -d "$rift_data" "$rift_url")
    else
        mb_result=$(curl -s -w $'\n%{http_code}' -X "$method" "$mb_url")
        rift_result=$(curl -s -w $'\n%{http_code}' -X "$method" "$rift_url")
    fi

    mb_status=$(echo "$mb_result" | tail -n1 | tr -d '\r')
    mb_body=$(echo "$mb_result" | sed '$d')

    rift_status=$(echo "$rift_result" | tail -n1 | tr -d '\r')
    rift_body=$(echo "$rift_result" | sed '$d')

    compare_status "$test_name" "$mb_status" "$rift_status"
}

# Test imposter endpoint
test_imposter() {
    local test_name="$1"
    local mb_port="$2"
    local method="$3"
    local path="$4"
    local data="$5"
    local headers="$6"

    local rift_port=$(get_rift_port "$mb_port")
    local mb_url="${MB_IMPOSTER_BASE}:${mb_port}${path}"
    local rift_url="${RIFT_IMPOSTER_BASE}:${rift_port}${path}"

    # Build curl commands - use proper escaping for newline separator
    local header_opt=""
    if [ -n "$headers" ]; then
        header_opt="-H \"$headers\""
    fi

    if [ -n "$data" ]; then
        mb_result=$(eval curl -s -w '$'\''\n%{http_code}'\' $header_opt -X "$method" -d "'$data'" "'$mb_url'")
        rift_result=$(eval curl -s -w '$'\''\n%{http_code}'\' $header_opt -X "$method" -d "'$data'" "'$rift_url'")
    else
        mb_result=$(eval curl -s -w '$'\''\n%{http_code}'\' $header_opt -X "$method" "'$mb_url'")
        rift_result=$(eval curl -s -w '$'\''\n%{http_code}'\' $header_opt -X "$method" "'$rift_url'")
    fi

    mb_status=$(echo "$mb_result" | tail -n1 | tr -d '\r')
    mb_body=$(echo "$mb_result" | sed '$d')

    rift_status=$(echo "$rift_result" | tail -n1 | tr -d '\r')
    rift_body=$(echo "$rift_result" | sed '$d')

    if compare_status "$test_name (status)" "$mb_status" "$rift_status"; then
        compare_json_response "$test_name (body)" "$mb_body" "$rift_body"
    fi
}

# =============================================================================
# Admin API Tests
# =============================================================================

test_admin_api() {
    log_section "Admin API Tests"

    # GET / - Root endpoint
    test_api "GET / (root)" "GET" "/" "" ""

    # GET /imposters - Empty list
    test_api "GET /imposters (empty)" "GET" "/imposters" "" ""

    # POST /imposters - Create single imposter
    local simple_imposter='{
        "port": 4545,
        "protocol": "http",
        "name": "Test Imposter",
        "stubs": [{
            "predicates": [{"equals": {"path": "/test"}}],
            "responses": [{"is": {"statusCode": 200, "body": "test"}}]
        }]
    }'
    test_api "POST /imposters (create)" "POST" "/imposters" "$simple_imposter" ""

    # GET /imposters/:port
    test_api "GET /imposters/4545" "GET" "/imposters/4545" "" ""

    # GET /imposters - List with one imposter
    test_api "GET /imposters (with imposter)" "GET" "/imposters" "" ""

    # DELETE /imposters/:port
    test_api "DELETE /imposters/4545" "DELETE" "/imposters/4545" "" ""

    # GET /imposters - Empty again
    test_api "GET /imposters (after delete)" "GET" "/imposters" "" ""

    # Test 404 for non-existent imposter
    test_api "GET /imposters/9999 (not found)" "GET" "/imposters/9999" "" ""
}

# =============================================================================
# Stub Management Tests
# =============================================================================

test_stub_management() {
    log_section "Stub Management Tests"

    # Create imposter for stub tests
    local imposter='{
        "port": 4545,
        "protocol": "http",
        "stubs": []
    }'
    local rift_imposter=$(remap_ports_for_rift "$imposter")
    curl -s -X POST -H "Content-Type: application/json" -d "$imposter" "$MB_ADMIN/imposters" > /dev/null
    curl -s -X POST -H "Content-Type: application/json" -d "$rift_imposter" "$RIFT_ADMIN/imposters" > /dev/null

    # Add stub
    local stub='{"stub": {"predicates": [], "responses": [{"is": {"statusCode": 200}}]}}'
    test_api "POST /imposters/4545/stubs (add)" "POST" "/imposters/4545/stubs" "$stub" ""

    # Replace stub
    local new_stub='{"predicates": [], "responses": [{"is": {"statusCode": 201}}]}'
    test_api "PUT /imposters/4545/stubs/0 (replace)" "PUT" "/imposters/4545/stubs/0" "$new_stub" ""

    # Delete stub
    test_api "DELETE /imposters/4545/stubs/0 (delete)" "DELETE" "/imposters/4545/stubs/0" "" ""

    # Cleanup
    curl -s -X DELETE "$MB_ADMIN/imposters/4545" > /dev/null
    curl -s -X DELETE "$RIFT_ADMIN/imposters/4545" > /dev/null
}

# =============================================================================
# Predicate Tests
# =============================================================================

test_predicates() {
    log_section "Predicate Tests"

    # Load test imposters
    local config=$(cat configs/basic-imposters.json)

    # Create imposters on both systems (with port remapping for Rift)
    local imposters=$(echo "$config" | jq -c '.imposters[]')
    while IFS= read -r imposter; do
        local rift_imposter=$(remap_ports_for_rift "$imposter")
        curl -s -X POST -H "Content-Type: application/json" -d "$imposter" "$MB_ADMIN/imposters" > /dev/null
        curl -s -X POST -H "Content-Type: application/json" -d "$rift_imposter" "$RIFT_ADMIN/imposters" > /dev/null
    done <<< "$imposters"

    sleep 1  # Wait for imposters to start

    # Test equals predicate
    test_imposter "equals (method+path)" 4545 "GET" "/hello" "" ""
    test_imposter "equals (POST)" 4545 "POST" "/echo" '{"data":"test"}' ""

    # Test startsWith predicate
    test_imposter "startsWith /api/" 4545 "GET" "/api/users" "" ""
    test_imposter "startsWith /api/v1" 4545 "GET" "/api/v1/items" "" ""

    # Test contains predicate
    test_imposter "contains search" 4545 "GET" "/search" "" ""
    test_imposter "contains search (path)" 4545 "GET" "/api/search/query" "" ""

    # Test matches (regex) predicate
    test_imposter "matches /users/123" 4545 "GET" "/users/123" "" ""
    test_imposter "matches /users/456" 4545 "GET" "/users/456" "" ""

    # Test default response (no match)
    test_imposter "default response" 4545 "GET" "/nonexistent" "" ""

    # Test header predicate
    test_imposter "header equals" 4546 "GET" "/" "" "X-Custom-Header: test-value"

    # Test query predicate
    test_imposter "query equals" 4546 "GET" "/?format=json" "" ""

    # Test AND predicate
    test_imposter "AND predicate" 4546 "GET" "/?page=1" "" ""

    # Cleanup
    curl -s -X DELETE "$MB_ADMIN/imposters" > /dev/null
    curl -s -X DELETE "$RIFT_ADMIN/imposters" > /dev/null
}

# =============================================================================
# Response Behavior Tests
# =============================================================================

test_response_behaviors() {
    log_section "Response Behavior Tests"

    # Create behavior test imposter (with port remapping for Rift)
    local config=$(cat configs/basic-imposters.json)
    local imposter=$(echo "$config" | jq '.imposters[2]')
    local rift_imposter=$(remap_ports_for_rift "$imposter")

    curl -s -X POST -H "Content-Type: application/json" -d "$imposter" "$MB_ADMIN/imposters" > /dev/null
    curl -s -X POST -H "Content-Type: application/json" -d "$rift_imposter" "$RIFT_ADMIN/imposters" > /dev/null

    sleep 1

    # Test wait behavior (timing)
    log_info "Testing wait behavior (should take ~100ms)..."
    local start_mb=$(date +%s%N)
    curl -s "http://localhost:4547/wait" > /dev/null
    local end_mb=$(date +%s%N)
    local mb_time=$(( (end_mb - start_mb) / 1000000 ))

    local start_rift=$(date +%s%N)
    curl -s "http://localhost:5547/wait" > /dev/null
    local end_rift=$(date +%s%N)
    local rift_time=$(( (end_rift - start_rift) / 1000000 ))

    if [ $mb_time -ge 90 ] && [ $rift_time -ge 90 ]; then
        log_success "wait behavior - Both services delayed (~${mb_time}ms MB, ~${rift_time}ms Rift)"
    else
        log_fail "wait behavior - Timing mismatch (${mb_time}ms MB, ${rift_time}ms Rift)"
    fi

    # Test response cycling
    log_info "Testing response cycling..."
    for i in 1 2 3 1 2 3; do
        test_imposter "cycle response $i" 4547 "GET" "/cycle" "" ""
    done

    # Cleanup
    curl -s -X DELETE "$MB_ADMIN/imposters" > /dev/null
    curl -s -X DELETE "$RIFT_ADMIN/imposters" > /dev/null
}

# =============================================================================
# Request Recording Tests
# =============================================================================

test_request_recording() {
    log_section "Request Recording Tests"

    # Create imposter with recording (with port remapping for Rift)
    local imposter='{
        "port": 4545,
        "protocol": "http",
        "recordRequests": true,
        "stubs": [{
            "predicates": [],
            "responses": [{"is": {"statusCode": 200, "body": "ok"}}]
        }]
    }'
    local rift_imposter=$(remap_ports_for_rift "$imposter")
    curl -s -X POST -H "Content-Type: application/json" -d "$imposter" "$MB_ADMIN/imposters" > /dev/null
    curl -s -X POST -H "Content-Type: application/json" -d "$rift_imposter" "$RIFT_ADMIN/imposters" > /dev/null

    sleep 1

    # Make some requests (using correct ports for each service)
    curl -s "http://localhost:4545/test1" > /dev/null
    curl -s "http://localhost:4545/test2" > /dev/null
    curl -s "http://localhost:5545/test1" > /dev/null
    curl -s "http://localhost:5545/test2" > /dev/null

    # Check recorded requests count (using correct ports for each service)
    mb_count=$(curl -s "$MB_ADMIN/imposters/4545" | jq '.numberOfRequests')
    rift_count=$(curl -s "$RIFT_ADMIN/imposters/4545" | jq '.numberOfRequests')

    if [ "$mb_count" == "$rift_count" ]; then
        log_success "Request count matches ($mb_count requests)"
    else
        log_fail "Request count differs (MB: $mb_count, Rift: $rift_count)"
    fi

    # Clear recorded requests
    test_api "DELETE /imposters/4545/savedRequests" "DELETE" "/imposters/4545/savedRequests" "" ""

    # Cleanup
    curl -s -X DELETE "$MB_ADMIN/imposters" > /dev/null
    curl -s -X DELETE "$RIFT_ADMIN/imposters" > /dev/null
}

# =============================================================================
# Batch Operations Tests
# =============================================================================

test_batch_operations() {
    log_section "Batch Operations Tests"

    # PUT /imposters - Replace all
    local batch='{
        "imposters": [
            {"port": 4545, "protocol": "http", "stubs": []},
            {"port": 4546, "protocol": "http", "stubs": []}
        ]
    }'
    test_api "PUT /imposters (batch replace)" "PUT" "/imposters" "$batch" ""

    # Verify both imposters exist
    test_api "GET /imposters (after batch)" "GET" "/imposters" "" ""

    # DELETE /imposters - Delete all
    test_api "DELETE /imposters (all)" "DELETE" "/imposters" "" ""

    # Verify empty
    test_api "GET /imposters (after delete all)" "GET" "/imposters" "" ""
}

# =============================================================================
# Proxy Tests
# =============================================================================

test_proxy() {
    log_section "Proxy Tests"

    # Create backend imposter on both systems (port 4546)
    local backend='{
        "port": 4546,
        "protocol": "http",
        "name": "Backend Service",
        "stubs": [
            {
                "predicates": [{"equals": {"path": "/api/users"}}],
                "responses": [{"is": {"statusCode": 200, "headers": {"Content-Type": "application/json"}, "body": {"users": [{"id": 1, "name": "Alice"}]}}}]
            },
            {
                "predicates": [{"equals": {"path": "/api/health"}}],
                "responses": [{"is": {"statusCode": 200, "body": "healthy"}}]
            },
            {
                "predicates": [],
                "responses": [{"is": {"statusCode": 200, "body": "backend default"}}]
            }
        ]
    }'
    curl -s -X POST -H "Content-Type: application/json" -d "$backend" "$MB_ADMIN/imposters" > /dev/null
    curl -s -X POST -H "Content-Type: application/json" -d "$backend" "$RIFT_ADMIN/imposters" > /dev/null

    sleep 1

    # Test 1: proxyOnce mode
    log_info "Testing proxyOnce mode..."
    local proxy_once='{
        "port": 4545,
        "protocol": "http",
        "name": "Proxy Once",
        "stubs": [{
            "responses": [{
                "proxy": {
                    "to": "http://localhost:4546",
                    "mode": "proxyOnce",
                    "predicateGenerators": [{"matches": {"path": true, "method": true}}]
                }
            }]
        }]
    }'
    curl -s -X POST -H "Content-Type: application/json" -d "$proxy_once" "$MB_ADMIN/imposters" > /dev/null
    curl -s -X POST -H "Content-Type: application/json" -d "$proxy_once" "$RIFT_ADMIN/imposters" > /dev/null

    sleep 1

    # First request (should proxy)
    curl -s http://localhost:4545/api/users > /dev/null
    curl -s http://localhost:5545/api/users > /dev/null

    # Check stubs were created
    mb_stubs=$(curl -s "$MB_ADMIN/imposters/4545" | jq '.stubs | length')
    rift_stubs=$(curl -s "$RIFT_ADMIN/imposters/4545" | jq '.stubs | length')

    if [ "$mb_stubs" == "$rift_stubs" ] && [ "$mb_stubs" -ge 2 ]; then
        log_success "proxyOnce - Stub created after first request"
    else
        log_fail "proxyOnce - Stub count mismatch (MB: $mb_stubs, Rift: $rift_stubs)"
    fi

    # Second request (should use recorded stub)
    mb_resp=$(curl -s http://localhost:4545/api/users)
    rift_resp=$(curl -s http://localhost:5545/api/users)

    if echo "$mb_resp" | grep -q "Alice" && echo "$rift_resp" | grep -q "Alice"; then
        log_success "proxyOnce - Recorded stub returns correct response"
    else
        log_fail "proxyOnce - Response mismatch"
    fi

    # Cleanup proxyOnce test
    curl -s -X DELETE "$MB_ADMIN/imposters/4545" > /dev/null
    curl -s -X DELETE "$RIFT_ADMIN/imposters/4545" > /dev/null

    # Test 2: proxyAlways mode
    log_info "Testing proxyAlways mode..."
    local proxy_always='{
        "port": 4545,
        "protocol": "http",
        "name": "Proxy Always",
        "stubs": [{
            "responses": [{
                "proxy": {
                    "to": "http://localhost:4546",
                    "mode": "proxyAlways",
                    "predicateGenerators": [{"matches": {"path": true}}]
                }
            }]
        }]
    }'
    curl -s -X POST -H "Content-Type: application/json" -d "$proxy_always" "$MB_ADMIN/imposters" > /dev/null
    curl -s -X POST -H "Content-Type: application/json" -d "$proxy_always" "$RIFT_ADMIN/imposters" > /dev/null

    sleep 1

    # Make 3 requests to same endpoint
    for i in 1 2 3; do
        curl -s http://localhost:4545/api/health > /dev/null
        curl -s http://localhost:5545/api/health > /dev/null
    done

    # Check that multiple responses were recorded
    mb_responses=$(curl -s "$MB_ADMIN/imposters/4545" | jq '.stubs[1].responses | length')
    rift_responses=$(curl -s "$RIFT_ADMIN/imposters/4545" | jq '.stubs[1].responses | length')

    if [ "$mb_responses" == "$rift_responses" ] && [ "$mb_responses" -ge 3 ]; then
        log_success "proxyAlways - Multiple responses recorded ($mb_responses)"
    else
        log_fail "proxyAlways - Response count mismatch (MB: $mb_responses, Rift: $rift_responses)"
    fi

    # Cleanup proxyAlways test
    curl -s -X DELETE "$MB_ADMIN/imposters/4545" > /dev/null
    curl -s -X DELETE "$RIFT_ADMIN/imposters/4545" > /dev/null

    # Test 3: addWaitBehavior
    log_info "Testing addWaitBehavior..."

    # Create backend with delay
    curl -s -X DELETE "$MB_ADMIN/imposters/4546" > /dev/null
    curl -s -X DELETE "$RIFT_ADMIN/imposters/4546" > /dev/null

    local slow_backend='{
        "port": 4546,
        "protocol": "http",
        "stubs": [{
            "responses": [{"is": {"statusCode": 200, "body": "slow"}, "_behaviors": {"wait": 100}}]
        }]
    }'
    curl -s -X POST -H "Content-Type: application/json" -d "$slow_backend" "$MB_ADMIN/imposters" > /dev/null
    curl -s -X POST -H "Content-Type: application/json" -d "$slow_backend" "$RIFT_ADMIN/imposters" > /dev/null

    local proxy_wait='{
        "port": 4545,
        "protocol": "http",
        "stubs": [{
            "responses": [{
                "proxy": {
                    "to": "http://localhost:4546",
                    "mode": "proxyOnce",
                    "addWaitBehavior": true,
                    "predicateGenerators": [{"matches": {"path": true}}]
                }
            }]
        }]
    }'
    curl -s -X POST -H "Content-Type: application/json" -d "$proxy_wait" "$MB_ADMIN/imposters" > /dev/null
    curl -s -X POST -H "Content-Type: application/json" -d "$proxy_wait" "$RIFT_ADMIN/imposters" > /dev/null

    sleep 1

    # Make request to record with latency
    curl -s http://localhost:4545/slow > /dev/null
    curl -s http://localhost:5545/slow > /dev/null

    # Check wait behavior was recorded (Mountebank uses 'behaviors', Rift uses '_behaviors')
    mb_wait=$(curl -s "$MB_ADMIN/imposters/4545" | jq '.stubs[0].responses[0].behaviors[0].wait // .stubs[0].responses[0]._behaviors.wait // 0')
    rift_wait=$(curl -s "$RIFT_ADMIN/imposters/4545" | jq '.stubs[0].responses[0]._behaviors.wait // .stubs[0].responses[0].behaviors[0].wait // 0')

    if [ "$mb_wait" -ge 90 ] && [ "$rift_wait" -ge 90 ]; then
        log_success "addWaitBehavior - Latency recorded (MB: ${mb_wait}ms, Rift: ${rift_wait}ms)"
    else
        log_fail "addWaitBehavior - Latency not recorded properly (MB: ${mb_wait}ms, Rift: ${rift_wait}ms)"
    fi

    # Cleanup
    curl -s -X DELETE "$MB_ADMIN/imposters" > /dev/null
    curl -s -X DELETE "$RIFT_ADMIN/imposters" > /dev/null
}

# =============================================================================
# Error Handling Tests
# =============================================================================

test_error_handling() {
    log_section "Error Handling Tests"

    # Invalid JSON
    test_api "POST /imposters (invalid JSON)" "POST" "/imposters" "not json" ""

    # Missing required field
    test_api "POST /imposters (missing port)" "POST" "/imposters" '{"protocol": "http"}' ""

    # Invalid protocol
    test_api "POST /imposters (invalid protocol)" "POST" "/imposters" '{"port": 4545, "protocol": "ftp"}' ""

    # Duplicate port - create imposter on 4545 for both, then try duplicate
    curl -s -X POST -H "Content-Type: application/json" -d '{"port": 4545}' "$MB_ADMIN/imposters" > /dev/null
    curl -s -X POST -H "Content-Type: application/json" -d '{"port": 4545}' "$RIFT_ADMIN/imposters" > /dev/null
    test_api "POST /imposters (duplicate port)" "POST" "/imposters" '{"port": 4545}' ""

    # Cleanup
    curl -s -X DELETE "$MB_ADMIN/imposters" > /dev/null
    curl -s -X DELETE "$RIFT_ADMIN/imposters" > /dev/null
}

# =============================================================================
# Main
# =============================================================================

main() {
    echo ""
    echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║     Mountebank vs Rift Compatibility Test Suite               ║${NC}"
    echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"
    echo ""

    wait_for_services

    # Clean slate
    curl -s -X DELETE "$MB_ADMIN/imposters" > /dev/null 2>&1
    curl -s -X DELETE "$RIFT_ADMIN/imposters" > /dev/null 2>&1

    # Run test suites
    test_admin_api
    test_stub_management
    test_predicates
    test_response_behaviors
    test_request_recording
    test_batch_operations
    test_proxy
    test_error_handling

    # Summary
    log_section "Test Summary"
    echo -e "  ${GREEN}Passed:${NC}  $TESTS_PASSED"
    echo -e "  ${RED}Failed:${NC}  $TESTS_FAILED"
    echo -e "  ${YELLOW}Skipped:${NC} $TESTS_SKIPPED"
    echo ""

    if [ $TESTS_FAILED -eq 0 ]; then
        echo -e "${GREEN}All tests passed!${NC}"
        exit 0
    else
        echo -e "${RED}Some tests failed.${NC}"
        exit 1
    fi
}

main "$@"
