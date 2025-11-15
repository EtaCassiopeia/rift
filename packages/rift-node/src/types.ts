/**
 * TypeScript types for Rift Node.js bindings
 * Provides Mountebank-compatible API types
 */

/**
 * Redis connection options for distributed state management
 */
export interface RedisOptions {
  /** Redis server hostname */
  host: string;
  /** Redis server port */
  port: number;
  /** Redis password (optional) */
  password?: string;
  /** Enable TLS for Redis connection */
  tls_enabled?: boolean;
}

/**
 * Options for creating a Rift server instance
 * Compatible with Mountebank's mb.create() options
 */
export interface CreateOptions {
  /** Admin API port (default: 2525) */
  port?: number;
  /** Bind address (default: localhost) */
  host?: string;
  /** Log level: debug, info, warn, error */
  loglevel?: 'debug' | 'info' | 'warn' | 'error';
  /** Path to log file */
  logfile?: string;
  /** IP addresses allowed to connect (Mountebank compatibility) */
  ipWhitelist?: string[];
  /** Enable JavaScript injection via Rhai scripts */
  allowInjection?: boolean;
  /** Path to custom imposters repository module */
  impostersRepository?: string;
  /** Redis configuration for distributed state */
  redis?: RedisOptions;
}

/**
 * Represents a running Rift server instance
 */
export interface RiftServer {
  /** The port the server is listening on */
  readonly port: number;
  /** The host the server is bound to */
  readonly host: string;
  /** Gracefully close the server */
  close(): Promise<void>;
}

/**
 * Mountebank imposter stub predicate
 */
export interface Predicate {
  equals?: Record<string, unknown>;
  deepEquals?: Record<string, unknown>;
  contains?: Record<string, unknown>;
  startsWith?: Record<string, unknown>;
  endsWith?: Record<string, unknown>;
  matches?: Record<string, unknown>;
  exists?: Record<string, unknown>;
  not?: Predicate;
  or?: Predicate[];
  and?: Predicate[];
  inject?: string;
}

/**
 * Mountebank imposter stub response
 */
export interface Response {
  is?: {
    statusCode?: number;
    headers?: Record<string, string>;
    body?: string | Record<string, unknown>;
  };
  proxy?: {
    to: string;
    mode?: 'proxyOnce' | 'proxyAlways' | 'proxyTransparent';
    predicateGenerators?: Array<{
      matches: Record<string, unknown>;
    }>;
  };
  inject?: string;
  _behaviors?: {
    wait?: number;
    repeat?: number;
    copy?: Array<{
      from: string;
      into: string;
      using: { method: string; selector: string };
    }>;
    decorate?: string;
  };
}

/**
 * Mountebank imposter stub
 */
export interface Stub {
  predicates?: Predicate[];
  responses: Response[];
}

/**
 * Mountebank imposter configuration
 */
export interface ImposterConfig {
  port: number;
  protocol: 'http' | 'https' | 'tcp' | 'smtp';
  name?: string;
  stubs?: Stub[];
  defaultResponse?: Response;
  allowCORS?: boolean;
  recordRequests?: boolean;
}

/**
 * Mountebank imposter with runtime state
 */
export interface Imposter extends ImposterConfig {
  numberOfRequests?: number;
  requests?: unknown[];
}

/**
 * Server information returned by GET /
 */
export interface ServerInfo {
  version: string;
  imposters: Array<{
    port: number;
    protocol: string;
    numberOfRequests?: number;
  }>;
}
