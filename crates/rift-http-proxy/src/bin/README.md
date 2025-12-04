# rift-verify - Stub Verification Tool

A CLI tool that automatically tests your imposters and stubs to verify they're working correctly. It fetches imposter configurations, generates test requests based on predicates, and verifies that responses match expectations.

### Build

```bash
# From the project root
cargo build --release --bin rift-verify

# The binary will be at:
# ./target/release/rift-verify
```

### Installation

After building, you can copy the binary to your PATH:

```bash
# Linux/macOS
sudo cp target/release/rift-verify /usr/local/bin/

# Or add to your local bin
cp target/release/rift-verify ~/.local/bin/
```

### Usage

```bash
# Verify all imposters on the default admin URL (http://localhost:2525)
rift-verify

# Specify a different admin URL
rift-verify --admin-url http://localhost:2525

# Verify a specific imposter by port
rift-verify --port 4545

# Show curl commands for each test (useful for debugging)
rift-verify --show-curl

# Verbose output with timing information
rift-verify --verbose

# Dry run - show what would be tested without making requests
rift-verify --dry-run

# Combine options
rift-verify -p 4545 --show-curl --verbose
```

### Command-Line Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--admin-url <URL>` | `-a` | Rift admin API URL | `http://localhost:2525` |
| `--port <PORT>` | `-p` | Verify specific imposter only | (all imposters) |
| `--show-curl` | `-c` | Show curl commands for each test | `false` |
| `--verbose` | `-v` | Verbose output with timing | `false` |
| `--timeout <SECS>` | `-t` | Request timeout in seconds | `10` |
| `--dry-run` | | Show tests without executing | `false` |
| `--skip-dynamic` | | Skip inject/proxy/script stubs | `true` |
| `--demo` | | Show enhanced error output examples | `false` |
| `--help` | `-h` | Print help information | |
| `--version` | `-V` | Print version | |

### How It Works

1. **Fetches Imposters**: Connects to the Rift admin API and retrieves all imposter configurations
2. **Parses Predicates**: Analyzes stub predicates to generate matching test requests
   - Supports: `equals`, `contains`, `startsWith`, `matches`, `exists`, `deepEquals`
3. **Sends Requests**: Makes HTTP requests to imposter ports with the generated test data
4. **Verifies Responses**: Compares actual responses against expected values:
   - Status code
   - Headers (expected headers must be present)
   - Body (supports JSON deep comparison)
5. **Reports Results**: Provides detailed output with failure information and optional curl commands

### Predicate Support

The tool generates test requests based on these predicate types:

| Predicate | Test Request Generation |
|-----------|------------------------|
| `equals` | Uses exact values specified |
| `contains` | Wraps value with prefix/suffix |
| `startsWith` | Uses the prefix value |
| `matches` (regex) | Generates sample matching value |
| `exists` | Adds header with test value |
| `deepEquals` | Uses exact values specified |
| `and` | Recursively parses all inner predicates |
| `or` | Uses first inner predicate |

### Skipped Stubs

By default, stubs with dynamic or stateful responses are skipped because their output cannot be predicted:

- **inject**: JavaScript injection responses
- **proxy**: Proxy responses to upstream servers
- **fault**: Fault injection responses
- **_rift.script**: Rift script-generated responses
- **cycling responses**: Stubs with multiple responses that rotate
- **repeat behavior**: Stubs with `_behaviors.repeat` (stateful)

Use `--skip-dynamic=false` to attempt verification of these stubs (results may be unpredictable).

### Enhanced Error Reporting

When verification fails, the tool provides detailed diagnostics:

- **Failure categorization**: Status mismatch, header mismatch, body mismatch, connection errors
- **Contextual hints**: Actionable suggestions based on the failure type
- **Unified diff**: For body mismatches, shows a line-by-line diff highlighting differences

Example failure output:
```
FAIL Stub #0 [user-api] - GET /api/users

   Why it failed:
   - Status mismatch: expected 200, got 404
     Hint: Got 404 instead of 200. The stub predicate may not match the test request path/method.
   - Body mismatch:
     Hint: Response body doesn't match. See diff below for details.
     Diff (-expected, +actual):
       {
         "users": [
           {"id": 1, "name": "Alice"},
     -     {"id": 2, "name": "Bob"}
     +     {"id": 3, "name": "Charlie"}
         ]
       }
```

Run `rift-verify --demo` to see examples of all failure types and their enhanced output.
