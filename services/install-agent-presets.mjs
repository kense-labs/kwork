/** Stage pinned upstream preset packages; profile activation is a separate commit. */
import { createHash, randomUUID } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const digest = bytes => createHash('sha256').update(bytes).digest('hex');

async function unpack(packageInfo, directory) {
  const response = await fetch(packageInfo.tarball, { signal: AbortSignal.timeout(30_000) });
  if (!response.ok) throw new Error(`Agent 预设依赖下载失败：${packageInfo.name} (${response.status})`);
  const bytes = Buffer.from(await response.arrayBuffer());
  const actual = 'sha512-' + createHash('sha512').update(bytes).digest('base64');
  if (actual !== packageInfo.integrity) throw new Error(`Agent 预设依赖完整性校验失败：${packageInfo.name}`);
  await mkdir(directory, { recursive: true });
  const archive = join(directory, '.archive.tgz');
  await writeFile(archive, bytes, { flag: 'wx', mode: 0o600 });
  try {
    const listing = spawnSync('tar', ['-tzf', archive], { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
    if (listing.error) throw listing.error;
    if (listing.status !== 0) throw Error(`预设依赖归档不可读：${packageInfo.name}`);
    const names = listing.stdout.trim().split('\n').map(name => name.replace(/^\.\//, ''));
    const prefix = names[0]?.split('/')[0];
    if (!prefix || !names.includes(prefix + '/package.json') || names.some(name =>
      name.startsWith('/') || name.split('/').includes('..') || name.split('/')[0] !== prefix)) {
      throw Error(`预设依赖归档目录无效：${packageInfo.name}`);
    }
    await new Promise((done, reject) => {
      // Pinned npm archives only. Do not invoke npm or any lifecycle scripts.
      // Older npm releases use a repository-generated prefix, not "package".
      const child = spawn('tar', ['-xzf', archive, '-C', directory, '--no-same-owner', '--strip-components', '1'],
        { stdio: ['ignore', 'ignore', 'pipe'] });
      let stderr = '';
      child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-4096); });
      child.once('error', reject);
      child.once('exit', code => code === 0 ? done() : reject(Error(`预设依赖解压失败：${packageInfo.name} (${code}) ${stderr}`)));
    });
  } finally { await rm(archive, { force: true }); }
  const manifest = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8'));
  if (manifest.name !== packageInfo.name || manifest.version !== packageInfo.version) {
    throw new Error(`Agent 预设依赖版本不匹配：${packageInfo.name}`);
  }
}

/** Download at most four immutable packages concurrently and publish one complete generation. */
export async function installAgentPresetPackages(target, lock) {
  lock ??= JSON.parse(await readFile(new URL('./vendor/dsh-agent-presets/packages.json', import.meta.url), 'utf8'));
  const lockText = JSON.stringify(lock);
  const generation = join(target, digest(lockText));
  const cached = async () => {
    try {
      if (await readFile(join(generation, 'complete.json'), 'utf8') !== lockText) throw Error('Agent 预设依赖缓存不匹配。');
      for (const entry of lock.packages) {
        const manifest = JSON.parse(await readFile(join(generation, 'node_modules', entry.name, 'package.json'), 'utf8'));
        if (manifest.name !== entry.name || manifest.version !== entry.version) throw Error('Agent 预设依赖缓存版本不匹配。');
      }
      return generation;
    } catch (error) { if (error.code === 'ENOENT') return undefined; throw error; }
  };
  const existing = await cached();
  if (existing) return existing;
  await mkdir(target, { recursive: true });
  const staging = join(target, '.install-' + randomUUID());
  await mkdir(staging);
  try {
    for (let start = 0; start < lock.packages.length; start += 4) {
      const results = await Promise.allSettled(lock.packages.slice(start, start + 4)
        .map(entry => unpack(entry, join(staging, 'node_modules', entry.name))));
      const failures = results.filter(result => result.status === 'rejected');
      if (failures.length) throw new AggregateError(failures.map(result => result.reason), 'Agent 预设依赖准备失败。');
    }
    await writeFile(join(staging, 'package.json'), JSON.stringify({ name: '@kense-labs/kwork-preset-runtime',
      private: true, type: 'module', dependencies: Object.fromEntries(lock.packages.map(entry => [entry.name, entry.version])) }), { mode: 0o600 });
    await writeFile(join(staging, 'complete.json'), lockText, { mode: 0o600 });
    await rename(staging, generation).catch(async error => {
      if (!['ENOTEMPTY', 'EEXIST'].includes(error.code) || !await cached()) throw error;
    });
    return await cached();
  } finally {
    // Every download settles before removing this invocation's unique staging directory.
    await rm(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!process.argv[2]) throw Error('Usage: install-agent-presets.mjs <owned-cache-directory>');
  console.log(await installAgentPresetPackages(resolve(process.argv[2])));
}
