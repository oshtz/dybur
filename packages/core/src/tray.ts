/**
 * Tray app management for dybur
 * Handles downloading and managing the tray application binary
 */

import {
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  rmSync,
  chmodSync,
} from 'fs';
import { join } from 'path';
import { exec } from 'child_process';
import { promisify } from 'util';
import {
  getBinDir,
  getTrayAppPath,
  getTrayAppBundlePath,
  getPlatform,
  getArch,
  isMacOS,
} from '@dybur/config';

const execAsync = promisify(exec);

/**
 * Tray app metadata stored alongside the binary
 */
export interface TrayAppMetadata {
  version: string;
  platform: string;
  arch: string;
  downloadedAt: string;
  source: string;
}

/**
 * Progress callback for downloads
 */
export type TrayDownloadProgress = (downloaded: number, total: number, status?: string) => void;

/**
 * GitHub repository for releases
 */
export const GITHUB_REPO = 'oshtz/dybur';
export const GITHUB_RELEASES_URL = `https://github.com/${GITHUB_REPO}/releases`;

/**
 * Current tray app version to download
 * Update this when releasing new versions
 */
export const TRAY_APP_VERSION = 'v1.0.0';

/**
 * Get the expected asset name for the current platform
 */
export function getTrayAssetName(): string {
  const platform = getPlatform();
  const arch = getArch();

  if (platform === 'darwin') {
    return `dybur-macos-${arch}.tar.gz`;
  }

  return `dybur-windows-${arch}.zip`;
}

/**
 * Get the download URL for the tray app
 */
export function getTrayDownloadUrl(version: string = TRAY_APP_VERSION): string {
  const assetName = getTrayAssetName();
  return `${GITHUB_RELEASES_URL}/download/${version}/${assetName}`;
}

/**
 * Ensure the bin directory exists
 */
export function ensureBinDir(): string {
  const dir = getBinDir();
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
  return dir;
}

/**
 * Check if the tray app is installed
 */
export function isTrayAppInstalled(): boolean {
  const trayPath = getTrayAppPath();
  return existsSync(trayPath);
}

/**
 * Get tray app metadata
 */
export function getTrayAppMetadata(): TrayAppMetadata | null {
  const metadataPath = join(getBinDir(), 'tray-metadata.json');

  if (!existsSync(metadataPath)) {
    return null;
  }

  try {
    return JSON.parse(readFileSync(metadataPath, 'utf-8')) as TrayAppMetadata;
  } catch {
    return null;
  }
}

/**
 * Download a file with progress tracking
 */
async function downloadFile(
  url: string,
  destPath: string,
  onProgress?: (downloaded: number, total: number) => void
): Promise<void> {
  const response = await fetch(url, {
    redirect: 'follow',
    headers: {
      'User-Agent': 'dybur-cli',
    },
  });

  if (!response.ok) {
    throw new Error(`Download failed: ${response.status} ${response.statusText}`);
  }

  const contentLength = parseInt(response.headers.get('content-length') ?? '0', 10);
  const reader = response.body?.getReader();

  if (!reader) {
    throw new Error('Failed to get response reader');
  }

  const fileStream = createWriteStream(destPath);
  let downloaded = 0;

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      fileStream.write(Buffer.from(value));
      downloaded += value.length;

      if (onProgress && contentLength > 0) {
        onProgress(downloaded, contentLength);
      }
    }
  } finally {
    fileStream.end();
    await new Promise<void>((resolve, reject) => {
      fileStream.on('finish', resolve);
      fileStream.on('error', reject);
    });
  }
}

/**
 * Extract a tar.gz archive (macOS)
 */
async function extractTarGz(archivePath: string, destDir: string): Promise<void> {
  await execAsync(`tar -xzf "${archivePath}" -C "${destDir}"`);
}

/**
 * Extract a zip archive (Windows)
 */
async function extractZip(archivePath: string, destDir: string): Promise<void> {
  // Use PowerShell's Expand-Archive on Windows
  await execAsync(
    `powershell -Command "Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}' -Force"`
  );
}

/**
 * Download and install the tray app from GitHub releases
 */
export async function downloadTrayApp(
  version: string = TRAY_APP_VERSION,
  onProgress?: TrayDownloadProgress
): Promise<string> {
  const platform = getPlatform();
  const arch = getArch();
  const binDir = ensureBinDir();
  const bundlePath = getTrayAppBundlePath();
  const trayPath = getTrayAppPath();

  // Remove existing installation if present
  if (existsSync(bundlePath)) {
    rmSync(bundlePath, { recursive: true, force: true });
  }

  const downloadUrl = getTrayDownloadUrl(version);
  const assetName = getTrayAssetName();
  const archivePath = join(binDir, assetName);

  try {
    // Download the archive
    if (onProgress) {
      onProgress(0, 0, 'Downloading tray application...');
    }

    await downloadFile(downloadUrl, archivePath, (downloaded, total) => {
      if (onProgress) {
        onProgress(downloaded, total);
      }
    });

    // Extract the archive
    if (onProgress) {
      onProgress(0, 0, 'Extracting...');
    }

    if (isMacOS()) {
      await extractTarGz(archivePath, binDir);

      // Make the binary executable
      if (existsSync(trayPath)) {
        chmodSync(trayPath, 0o755);
      }

      // Remove quarantine attribute on macOS
      try {
        await execAsync(`xattr -rd com.apple.quarantine "${bundlePath}"`);
      } catch {
        // Ignore if xattr fails (attribute might not exist)
      }
    } else {
      await extractZip(archivePath, binDir);
    }

    // Clean up the archive
    rmSync(archivePath, { force: true });

    // Verify extraction
    if (!existsSync(trayPath)) {
      throw new Error('Extraction failed: tray app binary not found');
    }

    // Write metadata
    const metadata: TrayAppMetadata = {
      version,
      platform,
      arch,
      downloadedAt: new Date().toISOString(),
      source: downloadUrl,
    };

    writeFileSync(join(binDir, 'tray-metadata.json'), JSON.stringify(metadata, null, 2));

    return trayPath;
  } catch (error) {
    // Clean up on failure
    if (existsSync(archivePath)) {
      rmSync(archivePath, { force: true });
    }
    if (existsSync(bundlePath)) {
      rmSync(bundlePath, { recursive: true, force: true });
    }
    throw error;
  }
}

/**
 * Check if a newer version is available
 */
export function isUpdateAvailable(): boolean {
  const metadata = getTrayAppMetadata();
  if (!metadata) {
    return true;
  }

  // Simple version comparison (could be enhanced with semver)
  return metadata.version !== TRAY_APP_VERSION;
}

/**
 * Remove the tray app installation
 */
export function removeTrayApp(): boolean {
  const bundlePath = getTrayAppBundlePath();
  const metadataPath = join(getBinDir(), 'tray-metadata.json');

  if (!existsSync(bundlePath)) {
    return false;
  }

  rmSync(bundlePath, { recursive: true, force: true });

  if (existsSync(metadataPath)) {
    rmSync(metadataPath, { force: true });
  }

  return true;
}
