/**
 * Binary discovery and download utilities for Rift
 */

import { execSync } from 'child_process';
import fs from 'fs';
import https from 'https';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Binary names in order of preference
// - rift-http-proxy: The original Cargo binary name
// - rift: The name used by install-local.sh
// - mb: Mountebank compatibility alias
const BINARY_NAMES: readonly string[] = process.platform === 'win32'
  ? ['rift-http-proxy.exe', 'rift.exe', 'mb.exe']
  : ['rift-http-proxy', 'rift', 'mb'];

const BINARY_NAME = BINARY_NAMES[0] as string;
const BINARIES_DIR = path.join(__dirname, '..', 'binaries');

/**
 * Platform to binary filename mapping
 */
export const PLATFORM_MAP: Record<string, string> = {
  'darwin-x64': 'rift-http-proxy-x86_64-apple-darwin.tar.gz',
  'darwin-arm64': 'rift-http-proxy-aarch64-apple-darwin.tar.gz',
  'linux-x64': 'rift-http-proxy-x86_64-unknown-linux-gnu.tar.gz',
  'linux-arm64': 'rift-http-proxy-aarch64-unknown-linux-gnu.tar.gz',
  'win32-x64': 'rift-http-proxy-x86_64-pc-windows-msvc.zip',
};

/**
 * Get the platform key for the current system
 */
export function getPlatformKey(): string {
  return `${process.platform}-${process.arch}`;
}

/**
 * Find the Rift binary path
 * Searches in order:
 * 1. RIFT_BINARY_PATH environment variable
 * 2. Local binaries directory (downloaded by postinstall)
 * 3. System PATH (checks for rift-http-proxy, rift, and mb)
 *
 * @returns Path to the Rift binary
 * @throws Error if binary not found
 */
export async function findBinary(): Promise<string> {
  // 1. Check RIFT_BINARY_PATH environment variable
  if (process.env.RIFT_BINARY_PATH) {
    const envPath = process.env.RIFT_BINARY_PATH;
    if (fs.existsSync(envPath)) {
      return envPath;
    }
    console.warn(`RIFT_BINARY_PATH set but file not found: ${envPath}`);
  }

  // 2. Check local binaries directory for any of the binary names
  for (const name of BINARY_NAMES) {
    const localBin = path.join(BINARIES_DIR, name);
    if (fs.existsSync(localBin)) {
      return localBin;
    }
  }

  // 3. Check system PATH for any of the binary names
  const whichCmd = process.platform === 'win32' ? 'where' : 'which';
  for (const name of BINARY_NAMES) {
    try {
      const result = execSync(`${whichCmd} ${name}`, {
        encoding: 'utf8',
        stdio: ['pipe', 'pipe', 'pipe'],
      });
      const binPath = result.trim().split('\n')[0];
      if (binPath && fs.existsSync(binPath)) {
        return binPath;
      }
    } catch {
      // Not found, try next name
    }
  }

  throw new Error(
    `Rift binary not found. Install it via one of:\n` +
      `  1. Run './scripts/install-local.sh' from the Rift repo\n` +
      `  2. Run 'npm install' again (postinstall will download)\n` +
      `  3. Set RIFT_BINARY_PATH environment variable\n` +
      `  4. Install 'rift' or 'rift-http-proxy' to your system PATH\n\n` +
      `For manual installation, visit: https://github.com/EtaCassiopeia/rift/releases`
  );
}

/**
 * Download a file from URL to destination
 */
async function downloadFile(url: string, dest: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);

    const request = https.get(url, (response) => {
      // Handle redirects
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
        fs.unlinkSync(dest);
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
      fs.unlinkSync(dest);
      reject(err);
    });

    file.on('error', (err) => {
      file.close();
      fs.unlinkSync(dest);
      reject(err);
    });
  });
}

/**
 * Extract a tar.gz file
 */
async function extractTarGz(archivePath: string, destDir: string): Promise<void> {
  // Use tar command for simplicity (available on macOS, Linux, and Git Bash on Windows)
  try {
    execSync(`tar -xzf "${archivePath}" -C "${destDir}"`, {
      stdio: 'pipe',
    });
  } catch (error) {
    throw new Error(`Failed to extract archive: ${error}`);
  }
}

/**
 * Extract a zip file (for Windows)
 */
async function extractZip(archivePath: string, destDir: string): Promise<void> {
  // Use PowerShell on Windows
  if (process.platform === 'win32') {
    try {
      execSync(
        `powershell -Command "Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}' -Force"`,
        { stdio: 'pipe' }
      );
    } catch (error) {
      throw new Error(`Failed to extract zip: ${error}`);
    }
  } else {
    // Use unzip on Unix systems
    try {
      execSync(`unzip -o "${archivePath}" -d "${destDir}"`, {
        stdio: 'pipe',
      });
    } catch (error) {
      throw new Error(`Failed to extract zip: ${error}`);
    }
  }
}

/**
 * Download and install the Rift binary for the current platform
 *
 * @param version Version to download (default: 'latest')
 * @param baseUrl Base URL for releases (default: GitHub releases)
 */
export async function downloadBinary(
  version: string = 'latest',
  baseUrl: string = 'https://github.com/EtaCassiopeia/rift/releases/download'
): Promise<string> {
  const platformKey = getPlatformKey();
  const filename = PLATFORM_MAP[platformKey];

  if (!filename) {
    throw new Error(
      `Unsupported platform: ${platformKey}\n` +
        `You can manually install the rift-http-proxy binary and set RIFT_BINARY_PATH`
    );
  }

  // Create binaries directory if it doesn't exist
  if (!fs.existsSync(BINARIES_DIR)) {
    fs.mkdirSync(BINARIES_DIR, { recursive: true });
  }

  const url = `${baseUrl}/${version}/${filename}`;
  const archivePath = path.join(BINARIES_DIR, filename);
  const binaryPath = path.join(BINARIES_DIR, BINARY_NAME);

  console.log(`Downloading Rift binary for ${platformKey}...`);
  console.log(`URL: ${url}`);

  // Download the archive
  await downloadFile(url, archivePath);

  // Extract the archive
  console.log('Extracting...');
  if (filename.endsWith('.tar.gz')) {
    await extractTarGz(archivePath, BINARIES_DIR);
  } else if (filename.endsWith('.zip')) {
    await extractZip(archivePath, BINARIES_DIR);
  }

  // Clean up archive
  fs.unlinkSync(archivePath);

  // Make binary executable (Unix only)
  if (process.platform !== 'win32') {
    fs.chmodSync(binaryPath, 0o755);
  }

  // Verify binary exists
  if (!fs.existsSync(binaryPath)) {
    throw new Error(`Binary not found after extraction: ${binaryPath}`);
  }

  console.log('Rift binary installed successfully!');
  return binaryPath;
}

/**
 * Check if the binary is already installed
 */
export function isBinaryInstalled(): boolean {
  try {
    findBinary();
    return true;
  } catch {
    return false;
  }
}

/**
 * Get the installed binary version
 */
export async function getBinaryVersion(): Promise<string | null> {
  try {
    const binaryPath = await findBinary();
    const result = execSync(`"${binaryPath}" --version`, {
      encoding: 'utf8',
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    return result.trim();
  } catch {
    return null;
  }
}
