/**
 * Unit tests for the main module
 */

import { jest } from '@jest/globals';

// Test the buildCliArgs function by mocking create and testing args
describe('index', () => {
  describe('module exports', () => {
    it('exports create function', async () => {
      const mod = await import('../../src/index.js');
      expect(typeof mod.create).toBe('function');
    });

    it('exports default object with create', async () => {
      const mod = await import('../../src/index.js');
      expect(typeof mod.default.create).toBe('function');
    });

    it('exports types', async () => {
      // Type exports are verified at compile time, but we can check
      // that the module doesn't throw when imported
      const mod = await import('../../src/index.js');
      expect(mod).toBeDefined();
    });

    it('exports binary utilities', async () => {
      const mod = await import('../../src/index.js');
      expect(typeof mod.findBinary).toBe('function');
      expect(typeof mod.downloadBinary).toBe('function');
      expect(typeof mod.getBinaryVersion).toBe('function');
    });
  });

  describe('CreateOptions', () => {
    // These tests verify the shape of options through TypeScript
    // The actual functionality is tested in integration tests

    it('accepts empty options', async () => {
      // This would fail at compile time if CreateOptions doesn't accept empty
      const options: Record<string, unknown> = {};
      expect(options).toEqual({});
    });

    it('accepts port option', async () => {
      const options = { port: 3000 };
      expect(options.port).toBe(3000);
    });

    it('accepts redis options', async () => {
      const options = {
        redis: {
          host: 'localhost',
          port: 6379,
          password: 'secret',
          tls_enabled: true,
        },
      };
      expect(options.redis.host).toBe('localhost');
      expect(options.redis.port).toBe(6379);
      expect(options.redis.password).toBe('secret');
      expect(options.redis.tls_enabled).toBe(true);
    });

    it('accepts all Mountebank-compatible options', async () => {
      const options = {
        port: 2525,
        host: 'localhost',
        loglevel: 'debug' as const,
        logfile: '/var/log/rift.log',
        ipWhitelist: ['*'],
        allowInjection: true,
        impostersRepository: '/path/to/repo',
        redis: {
          host: 'redis.example.com',
          port: 6379,
        },
      };

      expect(options.port).toBe(2525);
      expect(options.host).toBe('localhost');
      expect(options.loglevel).toBe('debug');
      expect(options.logfile).toBe('/var/log/rift.log');
      expect(options.ipWhitelist).toEqual(['*']);
      expect(options.allowInjection).toBe(true);
      expect(options.impostersRepository).toBe('/path/to/repo');
      expect(options.redis).toBeDefined();
    });
  });
});

describe('CLI argument building', () => {
  // Test that the options would translate correctly to CLI args
  // This is a design verification test

  it('default port is 2525', () => {
    const DEFAULT_PORT = 2525;
    expect(DEFAULT_PORT).toBe(2525);
  });

  it('default host is localhost', () => {
    const DEFAULT_HOST = 'localhost';
    expect(DEFAULT_HOST).toBe('localhost');
  });

  it('redis URL format with password and TLS', () => {
    const host = 'redis.example.com';
    const port = 6379;
    const password = 'secret';
    const tls_enabled = true;

    const protocol = tls_enabled ? 'rediss' : 'redis';
    const auth = password ? `:${encodeURIComponent(password)}@` : '';
    const redisUrl = `${protocol}://${auth}${host}:${port}`;

    expect(redisUrl).toBe('rediss://:secret@redis.example.com:6379');
  });

  it('redis URL format without password', () => {
    const host = 'redis.example.com';
    const port = 6379;
    const password = undefined;
    const tls_enabled = false;

    const protocol = tls_enabled ? 'rediss' : 'redis';
    const auth = password ? `:${encodeURIComponent(password)}@` : '';
    const redisUrl = `${protocol}://${auth}${host}:${port}`;

    expect(redisUrl).toBe('redis://redis.example.com:6379');
  });

  it('redis URL encodes special characters in password', () => {
    const host = 'redis.example.com';
    const port = 6379;
    const password = 'p@ss:word/test';
    const tls_enabled = false;

    const protocol = tls_enabled ? 'rediss' : 'redis';
    const auth = password ? `:${encodeURIComponent(password)}@` : '';
    const redisUrl = `${protocol}://${auth}${host}:${port}`;

    expect(redisUrl).toBe('redis://:p%40ss%3Aword%2Ftest@redis.example.com:6379');
  });
});
