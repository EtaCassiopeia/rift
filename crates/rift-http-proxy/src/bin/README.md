# Rift CLI Tools

This directory contains CLI tools for working with Rift imposters.

- [rift-tui](#rift-tui---interactive-terminal-ui) - Interactive terminal interface
- [rift-verify](#rift-verify---stub-verification-tool) - Test imposters by making requests
- [rift-lint](#rift-lint---configuration-linter) - Validate imposter configuration files

---

# rift-tui - Interactive Terminal UI

An interactive terminal user interface for managing Rift imposters and stubs. Built with Ratatui, it provides a visual way to manage your mock server without writing API calls.

### Build

```bash
# From the project root
cargo build --release --bin rift-tui

# The binary will be at:
# ./target/release/rift-tui
```

### Usage

```bash
# Connect to default admin URL (http://localhost:2525)
rift-tui

# Connect to a different admin URL
rift-tui --admin-url http://localhost:2525
```

### Features

- **Imposter Management**: Create, view, edit, and delete imposters
- **Stub Editor**: JSON editor with syntax validation and auto-formatting
- **Search & Filter**: Find imposters and stubs quickly with `/`
- **Import/Export**: Load and save imposter configurations
- **Curl Generation**: Generate curl commands from stubs with `y`
- **Metrics Dashboard**: View request counts and statistics
- **Vim-style Navigation**: Use j/k for navigation

### Key Bindings

| Key | Action |
|-----|--------|
| `j`/`k` | Move down/up |
| `Enter` | Select/drill down |
| `Esc` | Go back |
| `n` | New imposter |
| `p` | New proxy imposter |
| `a` | Add stub |
| `e` | Edit stub |
| `d` | Delete |
| `y` | Copy as curl |
| `/` | Search |
| `?` | Help |
| `q` | Quit |

For full documentation, see [Terminal UI](https://etacassiopeia.github.io/rift/features/tui/).

---

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
| `--status-only` | | Only verify status code (skip body/header checks) | `false` |
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

---

# rift-lint - Configuration Linter

A CLI tool that validates imposter configuration files before loading them into Rift. It detects common issues like port conflicts, invalid headers, JavaScript syntax errors, and malformed predicates.

### Build

```bash
# From the project root
cargo build --release --bin rift-lint

# The binary will be at:
# ./target/release/rift-lint
```

### Installation

After building, you can copy the binary to your PATH:

```bash
# Linux/macOS
sudo cp target/release/rift-lint /usr/local/bin/

# Or add to your local bin
cp target/release/rift-lint ~/.local/bin/
```

### Usage

```bash
# Lint all imposter files in a directory
rift-lint ./imposters/

# Lint a single file
rift-lint ./imposters/my-service.json

# Show only errors (hide warnings and info)
rift-lint ./imposters/ --errors-only

# JSON output for CI/CD integration
rift-lint ./imposters/ --output json

# Strict mode - treat warnings as errors
rift-lint ./imposters/ --strict

# Auto-fix issues where possible
rift-lint ./imposters/ --fix

# Verbose output
rift-lint ./imposters/ --verbose
```

### Command-Line Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `<PATH>` | | Path to imposter file or directory | (required) |
| `--fix` | `-f` | Auto-fix issues where possible | `false` |
| `--output <FORMAT>` | `-o` | Output format: `text` or `json` | `text` |
| `--errors-only` | `-e` | Only show errors (hide warnings) | `false` |
| `--verbose` | `-v` | Verbose output | `false` |
| `--strict` | `-s` | Treat warnings as errors | `false` |
| `--help` | `-h` | Print help information | |
| `--version` | `-V` | Print version | |

### Validation Rules

The linter checks for the following issues:

#### Errors (will prevent loading)

| Code | Description |
|------|-------------|
| E001 | Invalid JSON syntax |
| E002 | Port conflicts (multiple imposters on same port) |
| E003 | Missing required fields (`port`, `protocol`, `stubs`) |
| E004 | Invalid protocol (must be `http`, `https`, or `tcp`) |
| E005 | Port out of valid range (1-65535) |
| E006 | Stub missing `responses` field |
| E007 | Predicate must be an object |
| E008 | Predicate has no operator |
| E009 | Unknown predicate operator |
| E010 | Unbalanced brackets in JSONPath |
| E011 | JSONPath missing `selector` field |
| E012 | Invalid regex pattern |
| E014 | Response has no response type |
| E015 | Invalid HTTP status code |
| E016 | Invalid statusCode format |
| E017 | Empty header name |
| E018 | Header value is an array (must be string) |
| E019 | Header value is a number (must be string) |
| E020 | Header value is a boolean (must be string) |
| E021 | Headers must be an object |
| E022 | Proxy URL must start with http:// or https:// |
| E023 | Proxy `to` must be a string URL |
| E024 | Proxy missing required `to` field |
| E025 | Wait behavior must be number or function string |
| E026 | Unbalanced braces in JavaScript |
| E027 | Unbalanced parentheses in JavaScript |
| E028 | JavaScript syntax error |
| E029-E033 | Copy/Lookup behavior missing required fields |

#### Warnings (may cause issues)

| Code | Description |
|------|-------------|
| W001 | Privileged port (requires root) |
| W002 | Stub has no responses defined |
| W003 | Response has both `is` and `proxy` defined |
| W004 | Body is not valid JSON but Content-Type is application/json |
| W005 | Header value is null |
| W006 | Content-Length is very small |
| W007 | Unknown proxy mode |
| W008 | shellTransform contains potentially dangerous command |
| W009 | JavaScript behavior should be a function expression |

#### Info (informational)

| Code | Description |
|------|-------------|
| I001 | JSONPath uses Mountebank slice notation (supported but non-standard) |
| I002 | Proxy targets localhost (ensure upstream is running) |

### JSON Output

For CI/CD integration, use `--output json`:

```bash
rift-lint ./imposters/ --output json
```

```json
{
  "files_checked": 5,
  "errors": 2,
  "warnings": 1,
  "issues": [
    {
      "severity": "error",
      "code": "E019",
      "message": "Header 'Content-Length' value is a number, must be a string",
      "file": "/path/to/my-service.json",
      "location": "stubs[0].responses[0].is.headers.Content-Length",
      "suggestion": "Change to: \"Content-Length\": \"256\""
    }
  ]
}
```

### Auto-Fix

The `--fix` flag can automatically fix certain issues:

- **Header type conversion**: Arrays → comma-separated strings, numbers → strings, booleans → strings

```bash
rift-lint ./imposters/ --fix
```

### CI/CD Integration

```bash
# Exit code 0 = no errors, 1 = errors found
rift-lint ./imposters/ --strict || exit 1

# JSON for parsing
rift-lint ./imposters/ --output json > lint-results.json
```

