---
layout: default
title: Node.js Integration
parent: Getting Started
nav_order: 4
permalink: /getting-started/nodejs/
---

# Node.js Integration

Rift provides official Node.js bindings via the `@rift-vs/rift` npm package. This package is a drop-in replacement for Mountebank's Node.js API, making migration seamless.

---

## Installation

```bash
npm install @rift-vs/rift
```

The package automatically downloads the appropriate `rift-http-proxy` binary for your platform during installation.

### Supported Platforms

| Platform | Architecture |
|:---------|:-------------|
| macOS    | x64, arm64   |
| Linux    | x64, arm64   |
| Windows  | x64          |

---

## Basic Usage

```javascript
import rift from '@rift-vs/rift';

// Start a Rift server
const server = await rift.create({
  port: 2525,
  loglevel: 'info',
});

console.log(`Rift server running on ${server.host}:${server.port}`);

// Create an imposter via REST API
await fetch('http://localhost:2525/imposters', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    port: 4545,
    protocol: 'http',
    stubs: [{
      predicates: [{ equals: { path: '/api/users' } }],
      responses: [{
        is: {
          statusCode: 200,
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify([{ id: 1, name: 'Alice' }])
        }
      }]
    }]
  })
});

// Test your mock
const response = await fetch('http://localhost:4545/api/users');
console.log(await response.json()); // [{ id: 1, name: 'Alice' }]

// Clean up when done
await server.close();
```

---

## Migration from Mountebank

Migrating from Mountebank is straightforward - just change the import:

**Before (Mountebank):**

```javascript
import mb from 'mountebank';

const server = await mb.create({
  port: 2525,
  loglevel: 'debug',
  allowInjection: true,
});
```

**After (Rift):**

```javascript
import rift from '@rift-vs/rift';

const server = await rift.create({
  port: 2525,
  loglevel: 'debug',
  allowInjection: true,
});
```

All your existing imposter configurations and test code work without changes.

---

## TypeScript Support

Full TypeScript definitions are included:

```typescript
import rift, { CreateOptions, RiftServer } from '@rift-vs/rift';

const options: CreateOptions = {
  port: 2525,
  loglevel: 'debug',
};

const server: RiftServer = await rift.create(options);

// Type-safe properties
console.log(server.port); // number
console.log(server.host); // string

await server.close();
```

---

## API Reference

### `create(options?: CreateOptions): Promise<RiftServer>`

Creates and starts a new Rift server instance.

#### CreateOptions

| Option           | Type       | Default       | Description                         |
|:-----------------|:-----------|:--------------|:------------------------------------|
| `port`           | `number`   | `2525`        | Admin API port                      |
| `host`           | `string`   | `'localhost'` | Bind address                        |
| `loglevel`       | `string`   | `'info'`      | Log level: debug, info, warn, error |
| `logfile`        | `string`   | -             | Path to log file                    |
| `ipWhitelist`    | `string[]` | -             | Allowed IP addresses                |
| `allowInjection` | `boolean`  | `false`       | Enable script injection             |

#### RiftServer

| Property/Method | Type                  | Description                    |
|:----------------|:----------------------|:-------------------------------|
| `port`          | `number`              | Port the server is listening on |
| `host`          | `string`              | Host the server is bound to    |
| `close()`       | `Promise<void>`       | Gracefully shutdown the server |

#### Events

The server emits the following events:

- `exit` - Emitted when the server process exits
- `error` - Emitted on server errors
- `stdout` - Emitted with stdout data
- `stderr` - Emitted with stderr data

---

## Testing Example with Jest

```javascript
import rift from '@rift-vs/rift';

describe('API Tests', () => {
  let server;

  beforeAll(async () => {
    server = await rift.create({ port: 2525 });

    // Create test imposters
    await fetch('http://localhost:2525/imposters', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        port: 4545,
        protocol: 'http',
        stubs: [{
          predicates: [{ equals: { method: 'GET', path: '/health' } }],
          responses: [{ is: { statusCode: 200, body: { status: 'ok' } } }]
        }]
      })
    });
  });

  afterAll(async () => {
    await server.close();
  });

  test('health check returns ok', async () => {
    const response = await fetch('http://localhost:4545/health');
    const data = await response.json();
    expect(data.status).toBe('ok');
  });
});
```

---

## Environment Variables

| Variable                  | Description                                |
|:--------------------------|:-------------------------------------------|
| `RIFT_BINARY_PATH`        | Path to the rift-http-proxy binary         |
| `RIFT_VERSION`            | Version to download (default: latest)      |
| `RIFT_DOWNLOAD_URL`       | Custom download URL for binary             |
| `RIFT_SKIP_BINARY_DOWNLOAD` | Skip binary download during install      |

---

## Manual Binary Installation

If automatic download doesn't work (e.g., behind a firewall), install the binary manually:

1. Download from [GitHub Releases](https://github.com/EtaCassiopeia/rift/releases)
2. Set the `RIFT_BINARY_PATH` environment variable:

```bash
export RIFT_BINARY_PATH=/path/to/rift-http-proxy
npm install @rift-vs/rift
```

---

## Local Development & Testing

This section explains how to build and test Rift and the npm package locally before publishing.

### 1. Build Rift Binary

First, build the Rift binary from source:

```bash
# Clone the repository
git clone https://github.com/EtaCassiopeia/rift.git
cd rift

# Build the release binary
cargo build --release

# The binary is at: ./target/release/rift-http-proxy
```

### 2. Build the npm Package

```bash
cd packages/rift-node

# Install dependencies (skip binary download since we'll use local)
RIFT_SKIP_BINARY_DOWNLOAD=1 npm install

# Build TypeScript
npm run build
```

### 3. Test Locally with npm pack

This method creates a tarball that simulates a published package:

```bash
# Create the package tarball
cd packages/rift-node
npm pack
# Creates: rift-vs-rift-0.1.0.tgz

# In a test directory, install the tarball
mkdir /tmp/test-rift && cd /tmp/test-rift
npm init -y
npm install /path/to/rift/packages/rift-node/rift-vs-rift-0.1.0.tgz
```

### 4. Alternative: Test with npm link

For active development, `npm link` is faster:

```bash
# Link the package globally
cd packages/rift-node
npm link

# In your test directory, use the linked package
cd /tmp/test-rift
npm init -y
npm link @rift-vs/rift
```

### 5. Create a Test Script

Create `test.mjs` in your test directory:

```javascript
import rift from '@rift-vs/rift';

async function main() {
  console.log('Starting Rift server...');

  const server = await rift.create({
    port: 2525,
    loglevel: 'info',
  });

  console.log(`Server running on ${server.host}:${server.port}`);

  // Create a test imposter
  const createResponse = await fetch('http://localhost:2525/imposters', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      port: 4545,
      protocol: 'http',
      stubs: [{
        predicates: [{ equals: { path: '/test' } }],
        responses: [{ is: { statusCode: 200, body: 'Hello from Rift!' } }]
      }]
    })
  });

  console.log('Imposter created:', createResponse.status);

  // Test the imposter
  const testResponse = await fetch('http://localhost:4545/test');
  const body = await testResponse.text();
  console.log('Response:', body);

  // Verify
  if (body === 'Hello from Rift!') {
    console.log('✓ Test passed!');
  } else {
    console.error('✗ Test failed!');
    process.exit(1);
  }

  // Cleanup
  await server.close();
  console.log('Server stopped');
}

main().catch(err => {
  console.error('Error:', err);
  process.exit(1);
});
```

### 6. Run the Test

```bash
# Set the path to your locally built binary
export RIFT_BINARY_PATH=/path/to/rift/target/release/rift-http-proxy

# Run the test
node test.mjs
```

Expected output:

```
Starting Rift server...
Server running on localhost:2525
Imposter created: 201
Response: Hello from Rift!
✓ Test passed!
Server stopped
```

### Complete Local Test Script

For convenience, here's a script that does everything:

```bash
#!/bin/bash
set -e

RIFT_DIR="/path/to/rift"
TEST_DIR="/tmp/test-rift-local"

# Build Rift
echo "Building Rift..."
cd "$RIFT_DIR"
cargo build --release

# Build npm package
echo "Building npm package..."
cd "$RIFT_DIR/packages/rift-node"
RIFT_SKIP_BINARY_DOWNLOAD=1 npm install
npm run build
npm pack

# Setup test directory
echo "Setting up test..."
rm -rf "$TEST_DIR"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR"
npm init -y
npm install "$RIFT_DIR/packages/rift-node"/*.tgz

# Create test file
cat > test.mjs << 'EOF'
import rift from '@rift-vs/rift';

const server = await rift.create({ port: 2525 });
console.log('Server started on port', server.port);

await fetch('http://localhost:2525/imposters', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    port: 4545,
    protocol: 'http',
    stubs: [{
      predicates: [{ equals: { path: '/hello' } }],
      responses: [{ is: { statusCode: 200, body: 'Hello!' } }]
    }]
  })
});

const res = await fetch('http://localhost:4545/hello');
const text = await res.text();

if (text === 'Hello!') {
  console.log('✓ Test passed!');
} else {
  console.error('✗ Test failed:', text);
  process.exit(1);
}

await server.close();
EOF

# Run test
echo "Running test..."
export RIFT_BINARY_PATH="$RIFT_DIR/target/release/rift-http-proxy"
node test.mjs

echo "Done!"
```

---

## Utility Functions

### `findBinary(): Promise<string>`

Locates the rift-http-proxy binary. Searches in order:
1. `RIFT_BINARY_PATH` environment variable
2. Package's `binaries/` directory
3. System PATH

### `downloadBinary(version?: string): Promise<string>`

Downloads the Rift binary for the current platform.

### `getBinaryVersion(): Promise<string | null>`

Returns the installed binary version, or null if not found.

---

## Requirements

- Node.js 18.0.0 or later
- One of the supported platforms (see above)
