import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { access, copyFile, mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const nodeVersion = '24.22.3';
const nodeRelease = `node-v${nodeVersion}-win-x64`;
const nodeChecksum = 'e612af99f626ab415ea225af6bce5c971feb448cb5cae250bb1c3fa50c5f7d9f';
const services = fileURLToPath(new URL('../services/', import.meta.url));
const host = join(services, 'windows-host');
const stage = join(host, 'staging');
const bundle = join(host, 'bundle');

function verifyNodeArchive(bytes) {
  if (createHash('sha256').update(bytes).digest('hex') !== nodeChecksum) {
    throw new Error(`Node.js archive checksum mismatch for ${nodeRelease}`);
  }
}

async function prepareNode() {
  const archive = join(host, `${nodeRelease}.zip`);
  try {
    verifyNodeArchive(await readFile(archive));
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    const response = await fetch(`https://nodejs.org/dist/v${nodeVersion}/${nodeRelease}.zip`, {
      signal: AbortSignal.timeout(120_000),
    });
    if (!response.ok) throw new Error(`Node.js download returned HTTP ${response.status}`);
    const bytes = Buffer.from(await response.arrayBuffer());
    verifyNodeArchive(bytes);
    await writeFile(archive, bytes);
  }
  execFileSync('tar', ['-xf', archive, '-C', stage, '--strip-components', '1',
    `${nodeRelease}/node.exe`, `${nodeRelease}/LICENSE`], { stdio: 'inherit' });
  await rename(join(stage, 'LICENSE'), join(stage, 'LICENSE.nodejs.txt'));
}

async function prepareHarness() {
  // Reuse the official Host version and dependency lock from the macOS build.
  for (const file of ['package.json', 'bun.lock']) {
    await copyFile(join(services, 'macos-host', file), join(stage, file));
  }
  execFileSync(process.execPath, ['install', '--frozen-lockfile', '--linker', 'hoisted',
    '--ignore-scripts', '--backend=copyfile', '--os=win32', '--cpu=x64'], {
    cwd: stage, stdio: 'inherit',
  });
  for (const entry of ['bin.js', 'profile-boot.js']) {
    await access(join(stage, 'node_modules/@deepseek-ai/dsh/lib', entry));
  }
  // The application invokes the Host entry directly. The generated .bin links
  // point back into the staging path and are specific to the build machine.
  await rm(join(stage, 'node_modules', '.bin'), { recursive: true, force: true });
  await rm(join(stage, 'bun.lock'));
}

const terminalInstaller = fileURLToPath(new URL('../services/install-terminal-controller.mjs', import.meta.url));
const terminalTarget = join(stage, 'terminal-controller');
await rm(stage, { recursive: true, force: true });
await mkdir(stage, { recursive: true });
try {
  await prepareNode();
  await prepareHarness();
  execFileSync(process.execPath, [terminalInstaller, terminalTarget, stage, stage, '--prepare-only'], {
    stdio: 'inherit',
  });
  await rm(bundle, { recursive: true, force: true });
  await rename(stage, bundle);
} finally {
  await rm(stage, { recursive: true, force: true });
}
