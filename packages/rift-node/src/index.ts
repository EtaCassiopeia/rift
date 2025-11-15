/**
 * Rift Node.js bindings - Mountebank-compatible API
 *
 * This package provides a drop-in replacement for Mountebank's `mb.create()` API,
 * allowing easy migration from Mountebank to Rift.
 *
 * @example
 * ```javascript
 * import rift from 'rift-node';
 *
 * const server = await rift.create({
 *   port: 2525,
 *   loglevel: 'debug',
 *   redis: {
 *     host: 'localhost',
 *     port: 6379,
 *   },
 * });
 *
 * // Use the server...
 *
 * await server.close();
 * ```
 */

import { spawn, ChildProcess } from 'child_process';
import { EventEmitter } from 'events';
import axios, { AxiosError } from 'axios';
import { findBinary } from './binary.js';
import type { CreateOptions, RiftServer } from './types.js';

// Re-export types
export * from './types.js';
export { findBinary, downloadBinary, getBinaryVersion } from './binary.js';

const DEFAULT_PORT = 2525;
const DEFAULT_HOST = 'localhost';
const STARTUP_TIMEOUT_MS = 30000;
const HEALTH_CHECK_INTERVAL_MS = 100;
const SHUTDOWN_TIMEOUT_MS = 5000;

/**
 * Internal implementation of RiftServer
 */
class RiftServerImpl extends EventEmitter implements RiftServer {
  public readonly port: number;
  public readonly host: string;
  private process: ChildProcess | null;
  private closed: boolean = false;

  constructor(port: number, host: string, process: ChildProcess) {
    super();
    this.port = port;
    this.host = host;
    this.process = process;

    // Forward process events
    process.on('exit', (code, signal) => {
      this.emit('exit', code, signal);
    });

    process.on('error', (error) => {
      this.emit('error', error);
    });

    // Capture stdout/stderr for debugging
    process.stdout?.on('data', (data) => {
      this.emit('stdout', data.toString());
    });

    process.stderr?.on('data', (data) => {
      this.emit('stderr', data.toString());
    });
  }

  /**
   * Gracefully close the server
   */
  async close(): Promise<void> {
    if (this.closed || !this.process) {
      return;
    }

    this.closed = true;

    return new Promise((resolve) => {
      const timeout = setTimeout(() => {
        // Force kill if graceful shutdown takes too long
        this.process?.kill('SIGKILL');
        resolve();
      }, SHUTDOWN_TIMEOUT_MS);

      this.process!.once('exit', () => {
        clearTimeout(timeout);
        resolve();
      });

      // Send graceful shutdown signal
      this.process!.kill('SIGTERM');
    });
  }
}

/**
 * Build CLI arguments from CreateOptions
 */
function buildCliArgs(options: CreateOptions): string[] {
  const args: string[] = [];

  // Port
  const port = options.port || DEFAULT_PORT;
  args.push('--port', String(port));

  // Host/bind address
  if (options.host) {
    args.push('--host', options.host);
  }

  // Logging configuration
  if (options.loglevel) {
    args.push('--loglevel', options.loglevel);
  }
  if (options.logfile) {
    args.push('--log', options.logfile);
  }

  // Script injection support
  if (options.allowInjection) {
    args.push('--allow-injection');
  }

  // IP whitelist
  if (options.ipWhitelist && options.ipWhitelist.length > 0) {
    args.push('--ip-whitelist', options.ipWhitelist.join(','));
  }

  // Local only mode
  if (options.host === 'localhost' || options.host === '127.0.0.1') {
    // Don't add --local-only by default, let user control via ipWhitelist
  }

  // Note: Redis support for distributed state requires Rift native mode
  // The Mountebank API mode doesn't support Redis directly
  // For Redis support, use --rift-config with a native config file

  return args;
}

/**
 * Wait for the server to be ready by polling the health endpoint
 */
async function waitForServer(
  host: string,
  port: number,
  timeoutMs: number
): Promise<void> {
  const startTime = Date.now();
  const url = `http://${host}:${port}/`;

  while (Date.now() - startTime < timeoutMs) {
    try {
      await axios.get(url, { timeout: 1000 });
      // Server is responding
      return;
    } catch (error) {
      const axiosError = error as AxiosError;
      // If we get any HTTP response, the server is up
      if (axiosError.response) {
        return;
      }
      // Otherwise, wait and retry
      await sleep(HEALTH_CHECK_INTERVAL_MS);
    }
  }

  throw new Error(`Rift server did not start within ${timeoutMs}ms`);
}

/**
 * Sleep for a given number of milliseconds
 */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Create a new Rift server instance
 *
 * This function provides Mountebank-compatible API for creating a server.
 * It spawns the Rift binary as a child process and waits for it to be ready.
 *
 * @param options Server configuration options (compatible with Mountebank)
 * @returns A RiftServer instance that can be used to interact with the server
 *
 * @example
 * ```javascript
 * // Basic usage
 * const server = await create({ port: 2525 });
 *
 * // With Redis backend
 * const server = await create({
 *   port: 2525,
 *   redis: { host: 'localhost', port: 6379 }
 * });
 *
 * // Clean up
 * await server.close();
 * ```
 */
export async function create(options: CreateOptions = {}): Promise<RiftServer> {
  const port = options.port || DEFAULT_PORT;
  const host = options.host || DEFAULT_HOST;
  const args = buildCliArgs(options);

  // Find the rift binary
  const binaryPath = await findBinary();

  // Spawn the process
  const proc = spawn(binaryPath, args, {
    stdio: ['ignore', 'pipe', 'pipe'],
    detached: false,
  });

  // Capture stderr for error reporting
  let stderr = '';
  proc.stderr?.on('data', (data) => {
    stderr += data.toString();
  });

  // Handle spawn errors
  proc.on('error', (error) => {
    throw new Error(`Failed to start Rift: ${error.message}`);
  });

  // Check for early exit
  const earlyExitPromise = new Promise<never>((_, reject) => {
    proc.once('exit', (code, signal) => {
      if (code !== null && code !== 0) {
        reject(
          new Error(
            `Rift process exited with code ${code}.\nStderr: ${stderr || 'none'}`
          )
        );
      } else if (signal) {
        reject(new Error(`Rift process killed by signal ${signal}`));
      }
    });
  });

  // Wait for server to be ready, or handle early exit
  try {
    await Promise.race([
      waitForServer(host, port, STARTUP_TIMEOUT_MS),
      earlyExitPromise,
    ]);
  } catch (error) {
    proc.kill('SIGKILL');
    throw error;
  }

  return new RiftServerImpl(port, host, proc);
}

// Default export for Mountebank compatibility
export default { create };
