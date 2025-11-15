#!/usr/bin/env node

/**
 * Postinstall script for rift-node
 *
 * Downloads the appropriate Rift binary for the current platform.
 * This script runs automatically after `npm install`.
 */

import https from 'https';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import { execSync } from 'child_process';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const BINARIES_DIR = path.join(__dirname, '..', 'binaries');
const BINARY_NAME = process.platform === 'win32' ? 'rift.exe' : 'rift';

// Version to download (can be overridden by RIFT_VERSION env var)
// Use 'latest' to get the latest release, or specify a version like 'v0.1.0'
const VERSION = process.env.RIFT_VERSION || 'latest';

// GitHub releases base URL (can be overridden by RIFT_DOWNLOAD_URL env var)
const BASE_URL =
  process.env.RIFT_DOWNLOAD_URL || 'https://github.com/EtaCassiopeia/rift/releases/download';

// Platform to Rust target mapping
const PLATFORM_TO_TARGET = {
  'darwin-x64': 'x86_64-apple-darwin',
  'darwin-arm64': 'aarch64-apple-darwin',
  'linux-x64': 'x86_64-unknown-linux-gnu',
  'linux-arm64': 'aarch64-unknown-linux-gnu',
  'win32-x64': 'x86_64-pc-windows-msvc',
};

// Archive extension by platform
const ARCHIVE_EXT = {
  'darwin-x64': 'tar.gz',
  'darwin-arm64': 'tar.gz',
  'linux-x64': 'tar.gz',
  'linux-arm64': 'tar.gz',
  'win32-x64': 'zip',
};

/**
 * Download a file from URL, following redirects
 */
function downloadFile(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);

    const request = https.get(url, (response) => {
      // Handle redirects (GitHub releases redirect to S3)
      if (response.statusCode === 301 || response.statusCode === 302) {
        const redirectUrl = response.headers.location;
        if (redirectUrl) {
          file.close();
          fs.unlinkSync(dest);
          downloadFile(redirectUrl, dest).then(resolve).catch(reject);
          return;
        }
      }

      if (response.statusCode !== 200) {
        file.close();
        try {
          fs.unlinkSync(dest);
        } catch {}
        reject(new Error(`Failed to download: HTTP ${response.statusCode}`));
        return;
      }

      response.pipe(file);

      file.on('finish', () => {
        file.close();
        resolve();
      });
    });

    request.on('error', (err) => {
      file.close();
      try {
        fs.unlinkSync(dest);
      } catch {}
      reject(err);
    });

    file.on('error', (err) => {
      file.close();
      try {
        fs.unlinkSync(dest);
      } catch {}
      reject(err);
    });
  });
}

/**
 * Extract archive based on type
 */
function extractArchive(archivePath, destDir) {
  if (archivePath.endsWith('.tar.gz')) {
    execSync(`tar -xzf "${archivePath}" -C "${destDir}"`, { stdio: 'pipe' });
  } else if (archivePath.endsWith('.zip')) {
    if (process.platform === 'win32') {
      execSync(
        `powershell -Command "Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}' -Force"`,
        { stdio: 'pipe' }
      );
    } else {
      execSync(`unzip -o "${archivePath}" -d "${destDir}"`, { stdio: 'pipe' });
    }
  } else {
    throw new Error(`Unknown archive format: ${archivePath}`);
  }
}

/**
 * Get the latest release version from GitHub
 */
async function getLatestVersion() {
  return new Promise((resolve, reject) => {
    const options = {
      hostname: 'api.github.com',
      path: '/repos/EtaCassiopeia/rift/releases/latest',
      headers: {
        'User-Agent': 'rift-node-installer',
      },
    };

    https.get(options, (response) => {
      if (response.statusCode === 302 || response.statusCode === 301) {
        // Follow redirect
        https.get(response.headers.location, handleResponse);
        return;
      }
      handleResponse(response);

      function handleResponse(res) {
        let data = '';
        res.on('data', (chunk) => (data += chunk));
        res.on('end', () => {
          try {
            const json = JSON.parse(data);
            resolve(json.tag_name);
          } catch (e) {
            reject(new Error('Failed to parse GitHub API response'));
          }
        });
      }
    }).on('error', reject);
  });
}

async function main() {
  const platformKey = `${process.platform}-${process.arch}`;
  const target = PLATFORM_TO_TARGET[platformKey];
  const archiveExt = ARCHIVE_EXT[platformKey];

  // Check if binary already exists (e.g., user installed manually)
  const existingBinary = path.join(BINARIES_DIR, BINARY_NAME);
  if (fs.existsSync(existingBinary)) {
    console.log(`Rift binary already exists at ${existingBinary}`);
    return;
  }

  // Check if RIFT_BINARY_PATH is set (user has their own binary)
  if (process.env.RIFT_BINARY_PATH) {
    console.log(`Using existing binary from RIFT_BINARY_PATH: ${process.env.RIFT_BINARY_PATH}`);
    return;
  }

  // Check if running in CI with RIFT_SKIP_BINARY_DOWNLOAD
  if (process.env.RIFT_SKIP_BINARY_DOWNLOAD) {
    console.log('Skipping binary download (RIFT_SKIP_BINARY_DOWNLOAD is set)');
    return;
  }

  // Check if rift or mb is already in PATH
  const binaryNames = ['rift', 'mb', 'rift-http-proxy'];
  const whichCmd = process.platform === 'win32' ? 'where' : 'which';
  for (const name of binaryNames) {
    try {
      execSync(`${whichCmd} ${name}`, { stdio: 'pipe' });
      console.log(`Found '${name}' in PATH, skipping download`);
      return;
    } catch {
      // Not found, continue
    }
  }

  if (!target) {
    console.warn(`\nUnsupported platform: ${platformKey}`);
    console.warn('You can manually install the rift binary and set RIFT_BINARY_PATH');
    console.warn('Visit: https://github.com/EtaCassiopeia/rift/releases\n');
    // Don't fail the install - the package can still be used if binary is installed manually
    return;
  }

  // Create binaries directory
  if (!fs.existsSync(BINARIES_DIR)) {
    fs.mkdirSync(BINARIES_DIR, { recursive: true });
  }

  // Determine version to download
  let version = VERSION;
  if (version === 'latest') {
    try {
      console.log('Fetching latest release version...');
      version = await getLatestVersion();
      console.log(`Latest version: ${version}`);
    } catch (e) {
      console.warn(`Failed to get latest version: ${e.message}`);
      console.warn('Please set RIFT_VERSION environment variable or install manually');
      return;
    }
  }

  // Archive name format: rift-VERSION-TARGET.tar.gz (or .zip for Windows)
  const archiveName = `rift-${version}-${target}.${archiveExt}`;
  const url = `${BASE_URL}/${version}/${archiveName}`;
  const archivePath = path.join(BINARIES_DIR, archiveName);

  console.log(`\nDownloading Rift ${version} for ${platformKey}...`);
  console.log(`URL: ${url}\n`);

  try {
    // Download the archive
    await downloadFile(url, archivePath);

    // Extract the archive
    console.log('Extracting...');
    extractArchive(archivePath, BINARIES_DIR);

    // The archive contains a directory with the binary
    // Move the binary to the binaries directory
    const extractedDir = path.join(BINARIES_DIR, `rift-${version}-${target}`);
    const extractedBinary = path.join(extractedDir, BINARY_NAME);
    const finalBinaryPath = path.join(BINARIES_DIR, BINARY_NAME);

    if (fs.existsSync(extractedBinary)) {
      fs.renameSync(extractedBinary, finalBinaryPath);
      // Clean up extracted directory
      fs.rmSync(extractedDir, { recursive: true, force: true });
    } else if (fs.existsSync(finalBinaryPath)) {
      // Binary was extracted directly
    } else {
      throw new Error(`Binary not found after extraction`);
    }

    // Clean up archive file
    try {
      fs.unlinkSync(archivePath);
    } catch {}

    // Make binary executable (Unix only)
    if (process.platform !== 'win32') {
      fs.chmodSync(finalBinaryPath, 0o755);
    }

    console.log(`\nRift binary installed successfully!`);
    console.log(`Location: ${finalBinaryPath}\n`);
  } catch (error) {
    console.error(`\nFailed to download Rift binary: ${error.message}`);
    console.error('\nYou can manually install the rift binary:');
    console.error('  1. Download from: https://github.com/EtaCassiopeia/rift/releases');
    console.error('  2. Set RIFT_BINARY_PATH environment variable to the binary path');
    console.error('  3. Or place the binary in your system PATH\n');
    // Don't fail the install - the package can still be used if binary is installed manually
  }
}

main().catch((error) => {
  console.error('Postinstall error:', error);
  // Don't exit with error code - allow npm install to succeed
});
