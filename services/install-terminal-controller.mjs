import { createHash, randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { prepareTerminalProfile } from './agent-terminal-bridge.mjs';

const [target, service, home = join(homedir(), '.dsh')] = process.argv.slice(2);
const generation = join(target, 'v3');

const name = '@deepseek-ai/dsh-api-terminal-controller';
const packages = [name, '@deepseek-ai/dsh-terminal-panel', '@deepseek-ai/dsh-tool-terminal', '@deepseek-ai/dsh-terminal'];
const version = '0.2.2-rc.3';
const integrity = 'sha512-EPPCI0yVtxHUguodpddbmFz3Lwb/fbf5j7Bjq8Bl4xEeyblxRzfYtbyOSnbVKZHqRlbPp4f1I1w39ZcvVrbxBQ==';
// The ACP installation supplies Cordis, subprocess and session services. Keep
// the controller's Remote protocol in its own versioned runtime so its stream
// decorators and uplink iterator use one implementation.
const dependencies = [
  ['@deepseek-ai/dsh-terminal-panel', version, 'sha512-evA8R1Pl6lmlFm5Xk+pLYTuXmzfMctmril0d8xnnyPgXugiSEwvcoNlJNvWaz5mnzqFQN2/FUF8m94NTDMwZ6g=='],
  ['@deepseek-ai/dsh-tool-terminal', version, 'sha512-KQbBbBn3qBgE/iruxWCDZUVqXh5yMud6S8gnBL6J1PvaomiPGNXFm27Qpfc1KZmi9LXDv+H+bxbv0biWCptYww=='],
  ['@deepseek-ai/dsh-terminal', version, 'sha512-cqr/F8tQ89PpRAUYxBAERLw0/WRndH2WSuEw1bnqGFEX607a0pZf6EAPuR83K0slp2RPBTgvDRJ07KrEEJG36w=='],
  ['@deepseek-ai/dsh-output-retention', version, 'sha512-+EPNpuCoILswt3XK+9bhTGBnnl4+jN3bwWFL75W2Dd+6k5ul8Axnsam7eSH//3eLngxTAn/9WJAA/QdXCoBAUw=='],
  ['@deepseek-ai/dsh-brand', version, 'sha512-vX7Ugoqs9GTO/1r/R7K3wZ6Vq1erJHLPvzEr78yxMigCitM0PA4kBZ8i/WOZrsNTLUOu/OiiBqr9HW3CQ4jEww=='],
  ['@deepseek-ai/dsh-error', version, 'sha512-yxbiGjg8Y3LRIwZZBG5juutzZbVAHRZOPGTlSXaMtdBbnY6wbeV2BAnVnP9bGLb+Si+GKhbbQCA23RV/9+FMUQ=='],
  ['@deepseek-ai/dsh-deque', version, 'sha512-lw0MPTbwDm8YZJ4U2wNKlLRpvyGGZPKoj7VBocCeIx+VY9dOPDC/CUJQsgeu1acBlXlHpCv6vrHqYr7KWeb2tQ=='],
  ['@deepseek-ai/dsh-lazy-require', version, 'sha512-60KF/YeFXOolgqzFrYCUvF9DaYQengf4QOFx2LJ2njs9MYtqbMRBnXEnnmr6Xudcv3LFx/Kua3cGngucupDh1g=='],
  ['@deepseek-ai/dsh-typert-protocol', version, 'sha512-CXn6ILbfWN4a6NMsnIPPq22iVaKAWPb9522l01o903BqNM+hLWbSNipQ7myIUBRv4wn0Zw8ZgYGy9yrmfU4Htw=='],
  ['@deepseek-ai/dsh-util-values', version, 'sha512-WF8DOmVfOPLhybptIS9c+OompFiZzrMjWAxtCe9tbRNadVSRopf5eUtkspNU2Bsll2KzBc+Bz5ap8/DC1t70CQ=='],
  ['@deepseek-ai/schemastery', '3.19.0', 'sha512-bDXoYQa7ghAhmdYD5VPlzKxEzJV/aKkz5l4V5h1u06z8GAPN1ROewSjvR9WiXhpa52E9fng8lpH5QdCRIQgnyQ=='],
  ['@deepseek-ai/cosmokit', '1.9.0', 'sha512-5O1S53mZiJyZYcvxxD0Qv1EY7qFPSxbGyD8CbVK6gw7W9FvcXZj2YIX1ZkOqyQQ3Xt2rFkZDHJv0+V3Zmmp8CA=='],
  ['@standard-schema/spec', '1.1.0', 'sha512-l2aFy5jALhniG5HgqrD6jXLi/rUWrKvqN/qJx6yoJsgKhblVd+iqqU4RCXavm/jPityDo5TCvKMnpjKnOriy0w=='],
  ['@xterm/addon-serialize', '0.14.0', 'sha512-uteyTU1EkrQa2Ux6P/uFl2fzmXI46jy5uoQMKEOM0fKTyiW7cSn0WrFenHm5vO5uEXX/GpwW/FgILvv3r0WbkA=='],
  ['@xterm/headless', '6.0.0', 'sha512-5Yj1QINYCyzrZtf8OFIHi47iQtI+0qYFPHmouEfG8dHNxbZ9Tb9YGSuLcsEwj9Z+OL75GJqPyJbyoFer80a2Hw=='],
  ['js-yaml', '4.3.2', 'sha512-SFNOvSJ+Dgf/9An904Yx+CgSlIPCkIpao4qo51lpee25TIRejdH3rhR4EZMGoNx3/TP3O+wzWuiTFl4sqbltzA=='],
  ['argparse', '2.0.1', 'sha512-8+9WqebbFzpX9OR+Wa6O29asIogeRMzcGtAINdpMHHyAg10f05aSFVBbcEqGf/PXw1EjAZ+q2/bEBg3DvurK3Q=='],
  ['zod', '4.6.6', 'sha512-4UmzQUF5fxvfcYHlbjBmejU+w+hbDnWFx4aS8kBx7nPBHcO2SqPbpAG8snbdg+Xse4pRefR98hgjFgIMCI/UhA=='],
];

async function installed() {
  const root = generation;
  const manifest = JSON.parse(await readFile(join(root, 'package/package.json'), 'utf8'));
  if (manifest.name !== name || manifest.version !== version) throw new Error('终端模块缓存版本不匹配。');
  const entry = join(root, 'package/lib/index.js');
  if (!existsSync(entry)) throw Object.assign(new Error('终端模块缓存缺少入口。'), { code: 'ENOENT' });
  for (const packageName of packages.slice(1)) {
    const manifest = JSON.parse(await readFile(join(root, 'node_modules', packageName, 'package.json'), 'utf8'));
    if (manifest.name !== packageName || manifest.version !== version) {
      throw new Error(`终端依赖缓存不匹配：${packageName}。`);
    }
  }
  for (const [dependency, expected] of dependencies) {
    const actual = JSON.parse(await readFile(join(root, 'node_modules', dependency, 'package.json'), 'utf8'));
    if (actual.name !== dependency || actual.version !== expected) {
      throw new Error(`终端依赖缓存不匹配：${dependency}。`);
    }
  }
  return entry;
}

async function cached() {
  try {
    return await installed();
  } catch (error) {
    // A partial generation is recoverable. Do not hide malformed JSON, I/O
    // failures, or a mismatched package manifest behind a successful reinstall.
    if (error?.code === 'ENOENT') return;
    throw error;
  }
}

async function extract(archive, output, expected, digest) {
  await mkdir(output, { recursive: true });
  const response = await fetch(archive, { signal: AbortSignal.timeout(30_000) });
  if (!response.ok) throw new Error(`下载 Agent 终端模块失败：${archive}，HTTP ${response.status}。`);
  const bytes = Buffer.from(await response.arrayBuffer());
  const actual = `sha512-${createHash('sha512').update(bytes).digest('base64')}`;
  if (actual !== digest) throw new Error('Agent 终端模块完整性校验失败。');
  const temporary = join(output, '.archive');
  await writeFile(temporary, bytes, { flag: 'wx' });
  let result;
  try {
    // This script lives in KWork, outside the untrusted archive. All archive
    // paths are argv, and no interpreter or build tool runs in the extraction.
    result = spawnSync('tar', [
      '-xzf', temporary, '-C', output, '--no-same-owner', '--strip-components', '1', 'package',
    ], { cwd: process.cwd(), stdio: 'inherit' });
  } finally {
    await rm(temporary, { force: true });
  }
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error('Agent 终端模块解压失败。');
  const manifest = JSON.parse(await readFile(join(output, 'package.json'), 'utf8'));
  if (manifest.name !== expected[0] || manifest.version !== expected[1]) {
    throw new Error('Agent 终端模块版本不匹配。');
  }
}

export async function main() {
  let entry = await cached();
  if (!entry) {
    await mkdir(target, { recursive: true });
    const staging = join(target, `.install-${randomUUID()}`);
    await mkdir(staging, { recursive: false });
    try {
      await extract(
        `https://registry.npmjs.org/${name}/-/${name.split('/')[1]}-${version}.tgz`,
        join(staging, 'package'), [name, version], integrity,
      );
      if (!existsSync(join(staging, 'package/lib/index.js'))) throw new Error('Agent 终端模块缺少入口。');
      for (const [dependency, release, digest] of dependencies) {
        const archive = `https://registry.npmjs.org/${dependency}/-/${dependency.split('/').at(-1)}-${release}.tgz`;
        await extract(archive, join(staging, 'node_modules', dependency), [dependency, release], digest);
      }
      // Recheck before replacing anything: another process may have completed
      // the same generation while this staging directory was downloading.
      if (!await cached()) {
        await rename(staging, generation).catch(async error => {
          if (error?.code !== 'ENOTEMPTY' && error?.code !== 'EEXIST') throw error;
          if (await cached()) return;
          // A previous failed installation can leave only KWork's incomplete
          // generation. Move it aside atomically; never remove a complete cache.
          const incomplete = join(target, `.incomplete-${randomUUID()}`);
          await rename(generation, incomplete);
          await rename(staging, generation);
          await rm(incomplete, { recursive: true, force: true });
        });
      }
      entry = await cached();
      if (!entry) throw new Error('Agent 终端模块安装未完成。');
    } catch (error) {
      entry = await cached();
      if (!entry) throw error;
    } finally {
      await rm(staging, { recursive: true, force: true });
    }
  }
  await prepareTerminalProfile({ service, module: entry, home, runtime: dirname(dirname(dirname(entry))) });
  return entry;
}
