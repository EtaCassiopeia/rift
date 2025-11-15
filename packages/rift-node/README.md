# rift-node

Mountebank-compatible Node.js bindings for [Rift](https://github.com/EtaCassiopeia/rift) - a high-performance service virtualization and chaos engineering proxy written in Rust.

## Features

- **Drop-in replacement** for Mountebank's `mb.create()` API
- **High performance** - Rust-based proxy with minimal resource usage
- **Fault injection** - Built-in chaos engineering capabilities
- **TypeScript support** - Full type definitions included
- **Cross-platform** - Supports macOS, Linux, and Windows

## Installation

```bash
npm install rift-node
```

The package automatically downloads the appropriate `rift-http-proxy` binary for your platform during installation.

### Supported Platforms

- macOS (x64, arm64)
- Linux (x64, arm64)
- Windows (x64)

### Manual Binary Installation

If automatic download doesn't work, you can install the binary manually:

1. Download from [GitHub Releases](https://github.com/EtaCassiopeia/rift/releases)
2. Set the `RIFT_BINARY_PATH` environment variable to the binary location

### Local Development Installation

If you're working with the Rift source code, you can install locally:

```bash
# Clone and build Rift
git clone https://github.com/EtaCassiopeia/rift.git
cd rift

# Install the binary locally (builds and installs to ~/.local/bin)
./scripts/install-local.sh

# Or if you already have a release build:
RIFT_SKIP_BUILD=1 ./scripts/install-local.sh
```

The package will automatically find the `rift` or `mb` binary in your PATH.

## Usage

### Basic Usage

```javascript
import rift from 'rift-node';

// Start a Rift server (Mountebank-compatible)
const server = await rift.create({
  port: 2525,
  loglevel: 'debug',
});

// Create an imposter via REST API
await fetch('http://localhost:2525/imposters', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    port: 4545,
    protocol: 'http',
    stubs: [
      {
        predicates: [{ equals: { path: '/api/users' } }],
        responses: [
          {
            is: {
              statusCode: 200,
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify([{ id: 1, name: 'Alice' }]),
            },
          },
        ],
      },
    ],
  }),
});

// Your tests can now use http://localhost:4545/api/users

// Clean up
await server.close();
```

### Migration from Mountebank

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
import rift from 'rift-node';

const server = await rift.create({
  port: 2525,
  loglevel: 'debug',
  allowInjection: true,
});
```

### TypeScript

Full TypeScript support is included:

```typescript
import rift, { CreateOptions, RiftServer } from 'rift-node';

const options: CreateOptions = {
  port: 2525,
  loglevel: 'debug',
};

const server: RiftServer = await rift.create(options);

// server.port - the port the server is listening on
// server.host - the host the server is bound to
// server.close() - gracefully close the server
```

## API Reference

### `create(options?: CreateOptions): Promise<RiftServer>`

Creates and starts a new Rift server instance.

#### Options

| Option               | Type         | Default       | Description                                |
| -------------------- | ------------ | ------------- | ------------------------------------------ |
| `port`               | `number`     | `2525`        | Admin API port                             |
| `host`               | `string`     | `'localhost'` | Bind address                               |
| `loglevel`           | `string`     | `'info'`      | Log level: debug, info, warn, error        |
| `logfile`            | `string`     | -             | Path to log file                           |
| `ipWhitelist`        | `string[]`   | -             | Allowed IP addresses                       |
| `allowInjection`     | `boolean`    | `false`       | Enable script injection                    |

### `RiftServer`

The server instance returned by `create()`.

#### Properties

- `port: number` - The port the server is listening on
- `host: string` - The host the server is bound to

#### Methods

- `close(): Promise<void>` - Gracefully shutdown the server

#### Events

- `exit` - Emitted when the server process exits
- `error` - Emitted on server errors
- `stdout` - Emitted with stdout data
- `stderr` - Emitted with stderr data

### Utility Functions

#### `findBinary(): Promise<string>`

Locates the rift-http-proxy binary. Searches in order:
1. `RIFT_BINARY_PATH` environment variable
2. Package's `binaries/` directory
3. System PATH

#### `downloadBinary(version?: string): Promise<string>`

Downloads the Rift binary for the current platform.

#### `getBinaryVersion(): Promise<string | null>`

Returns the installed binary version, or null if not found.

## REST API Compatibility

Rift implements the Mountebank REST API:

| Endpoint                          | Method   | Description           |
| --------------------------------- | -------- | --------------------- |
| `/`                               | GET      | Server info           |
| `/imposters`                      | GET      | List all imposters    |
| `/imposters`                      | POST     | Create imposter       |
| `/imposters`                      | PUT      | Replace all imposters |
| `/imposters`                      | DELETE   | Delete all imposters  |
| `/imposters/:port`                | GET      | Get imposter          |
| `/imposters/:port`                | DELETE   | Delete imposter       |
| `/imposters/:port/stubs`          | POST     | Add stub to imposter  |
| `/imposters/:port/requests`       | DELETE   | Clear requests        |

## Environment Variables

| Variable                  | Description                                      |
| ------------------------- | ------------------------------------------------ |
| `RIFT_BINARY_PATH`        | Path to the rift-http-proxy binary               |
| `RIFT_VERSION`            | Version to download (default: latest)            |
| `RIFT_DOWNLOAD_URL`       | Custom download URL for binary                   |
| `RIFT_SKIP_BINARY_DOWNLOAD` | Skip binary download during install            |

## Requirements

- Node.js 18.0.0 or later
- One of the supported platforms (see above)

## License

MIT

## Related

- [Rift](https://github.com/EtaCassiopeia/rift) - The Rust proxy
- [Mountebank](https://www.mbtest.org/) - The original service virtualization tool
