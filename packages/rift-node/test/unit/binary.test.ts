/**
 * Unit tests for binary discovery and download utilities
 */

import { jest } from '@jest/globals';
import { PLATFORM_MAP, getPlatformKey } from '../../src/binary.js';

describe('binary', () => {
  describe('getPlatformKey', () => {
    it('returns correct format for current platform', () => {
      const key = getPlatformKey();
      expect(key).toMatch(/^(darwin|linux|win32)-(x64|arm64)$/);
    });
  });

  describe('PLATFORM_MAP', () => {
    it('has entries for common platforms', () => {
      expect(PLATFORM_MAP['darwin-x64']).toBeDefined();
      expect(PLATFORM_MAP['darwin-arm64']).toBeDefined();
      expect(PLATFORM_MAP['linux-x64']).toBeDefined();
      expect(PLATFORM_MAP['linux-arm64']).toBeDefined();
      expect(PLATFORM_MAP['win32-x64']).toBeDefined();
    });

    it('darwin binaries have tar.gz extension', () => {
      expect(PLATFORM_MAP['darwin-x64']).toMatch(/\.tar\.gz$/);
      expect(PLATFORM_MAP['darwin-arm64']).toMatch(/\.tar\.gz$/);
    });

    it('linux binaries have tar.gz extension', () => {
      expect(PLATFORM_MAP['linux-x64']).toMatch(/\.tar\.gz$/);
      expect(PLATFORM_MAP['linux-arm64']).toMatch(/\.tar\.gz$/);
    });

    it('windows binary has zip extension', () => {
      expect(PLATFORM_MAP['win32-x64']).toMatch(/\.zip$/);
    });
  });
});
