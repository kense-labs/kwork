import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { access, copyFile, mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const hostSource = fileURLToPath(new URL('../services/macos-host/', import.meta.url));
const terminalInstaller = fileURLToPath(new URL('../services/install-terminal-controller.mjs', import.meta.url));
const nodeVersion = '24.22.3';
const nodeChecksums = {
  arm64: 'ff6e17d4874b76d272212b908cc020d6e815fba5a6c7eaaa06add725840307fd',
  x64: '977614af0504c81045e7fd85596dbca0e5f951b96681516dadafedcca8c447f7',
};

function installHost(cwd, options = []) {
  const install = spawnSync(process.execPath, ['install', '--frozen-lockfile', '--linker', 'hoisted',
    '--ignore-scripts', ...options], { cwd, stdio: 'inherit' });
  if (install.error) throw install.error;
  if (install.status !== 0) throw new Error(`Harness installation failed in ${cwd}`);
}

async function prepareNode(directory, stage) {
  const checksum = nodeChecksums[process.arch];
  if (!checksum) throw new Error(`Unsupported macOS architecture: ${process.arch}`);
  const release = `node-v${nodeVersion}-darwin-${process.arch}`;
  const archive = join(directory, `${release}.tar.gz`);
  const bytes = await readFile(archive).catch(async error => {
    if (error.code !== 'ENOENT') throw error;
    const response = await fetch(`https://nodejs.org/dist/v${nodeVersion}/${release}.tar.gz`, {
      signal: AbortSignal.timeout(120_000),
    });
    if (!response.ok) throw new Error(`Node.js download returned HTTP ${response.status}`);
    return Buffer.from(await response.arrayBuffer());
  });
  if (createHash('sha256').update(bytes).digest('hex') !== checksum) {
    throw new Error(`Node.js archive checksum mismatch for ${release}`);
  }
  await writeFile(archive, bytes);
  execFileSync('tar', ['-xzf', archive, '-C', stage, '--strip-components', '1',
    `${release}/bin/node`, `${release}/LICENSE`], { stdio: 'inherit' });
  await rename(join(stage, 'bin/node'), join(stage, 'node'));
  await rm(join(stage, 'bin'), { recursive: true });
  await rename(join(stage, 'LICENSE'), join(stage, 'LICENSE.nodejs.txt'));
}

async function prepareBundle() {
  const host = fileURLToPath(new URL('../.tools/macos-host/', import.meta.url));
  const bundle = join(host, 'bundle');
  await mkdir(host, { recursive: true });
  const stage = await mkdtemp(join(host, 'stage-'));
  try {
    await prepareNode(host, stage);
    for (const file of ['package.json', 'bun.lock']) {
      await copyFile(join(hostSource, file), join(stage, file));
    }
    installHost(stage, ['--backend=copyfile']);
    for (const entry of ['bin.js', 'profile-boot.js']) {
      await access(join(stage, 'node_modules/@deepseek-ai/dsh/lib', entry));
    }
    await rm(join(stage, 'node_modules/.bin'), { recursive: true, force: true });
    await rm(join(stage, 'bun.lock'));
    execFileSync(process.execPath, [terminalInstaller, join(stage, 'terminal-controller'),
      stage, stage, '--prepare-only'], { stdio: 'inherit' });
    await rm(bundle, { recursive: true, force: true });
    await rename(stage, bundle);
  } finally {
    await rm(stage, { recursive: true, force: true });
  }
}

if (process.platform === 'darwin') {
  if (process.argv.includes('--bundle')) {
    await prepareBundle();
  } else if (!process.env.DSH_ACP_PATH) {
    installHost(hostSource);
  }
}
