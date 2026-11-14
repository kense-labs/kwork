import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, readdir, rename, rm, stat, writeFile } from 'node:fs/promises';
import { rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

const installer = new URL('../services/install-terminal-controller.mjs', import.meta.url).pathname;
const catalogSource = await readFile(installer, 'utf8');
const digests = new Map();
const catalogBody = catalogSource.slice(
  catalogSource.indexOf('const name ='), catalogSource.indexOf('async function installed()'),
);
const { name: controllerName, version, packages, dependencies, integrity } = new Function(
  `${catalogBody}\nreturn { name, version, packages, dependencies, integrity };`,
)();
const releases = new Map([
  [controllerName, { version, integrity }],
  ...dependencies.map(([name, version, integrity]) => [name, { version, integrity }]),
]);
for (const name of packages) assert(releases.has(name), `${name} is missing from the dependency catalog`);

const archives = await mkdtemp(join(tmpdir(), 'kwork-terminal-archives-'));
test.after(() => rmSync(archives, { recursive: true, force: true }));
// The fixture source is built after the archive loop below has populated the
// integrity map, so the replacement cannot run before its lookup data exists.

function archiveName(name, release) {
  return join(archives, `${name.split('/').at(-1)}-${release}.tgz`);
}

function createArchive(name, release) {
  const directory = `fixture-${name.split('/').at(-1)}-${release}`;
  const result = spawnSync('tar', [
    '-czf', '-', '--format', 'ustar', '-C', join(archives, directory), 'package',
  ], { maxBuffer: 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr.toString());
  return result.stdout;
}

for (const [name, { version: release, integrity: expected }] of releases) {
  const directory = join(archives, `fixture-${name.split('/').at(-1)}-${release}`, 'package');
  await mkdir(join(directory, 'lib'), { recursive: true });
  await writeFile(join(directory, 'package.json'), JSON.stringify({ name, version: release }));
  await writeFile(join(directory, 'lib/index.js'), `export const packageName = ${JSON.stringify(name)};\n`);
  const bytes = createArchive(name, release);
  digests.set(expected, `sha512-${createHash('sha512').update(bytes).digest('base64')}`);
  await writeFile(archiveName(name, release), bytes);
}

async function workspace() {
  const root = await mkdtemp(join(tmpdir(), 'kwork-terminal-install-'));
  const target = join(root, 'cache');
  const home = join(root, 'home');
  const service = join(root, 'service');
  const log = join(root, 'preparations.log');
  await mkdir(service, { recursive: true });
  const adapter = join(root, 'adapter.mjs');
  await writeFile(adapter, `import { appendFile } from 'node:fs/promises';
export async function prepareTerminalProfile(options) {
  await appendFile(${JSON.stringify(log)}, JSON.stringify(options) + '\\n');
}
`);
  const source = catalogSource.replace(/sha512-[A-Za-z0-9+/=]+/g,
    digest => { if (!digests.has(digest)) throw new Error(`Unmapped integrity: ${digest}`); return digests.get(digest); })
    .replace("'./agent-terminal-bridge.mjs'", JSON.stringify(adapter))
    .replace('const generation = join(target, \'v3\');', `const generation = join(target, 'v3');
globalThis.fetch = async url => {
  const path = String(url).replace('https://registry.npmjs.org/', '');
  const bytes = await readFile(join(${JSON.stringify(archives)}, path.split('/').at(-1)));
  return new Response(bytes);
};
`);
  const entry = join(root, 'installer.mjs');
  await writeFile(entry, `${source}\nconsole.log(await main());\n`);
  return { root, target, home, service, entry, log };
}

async function install(h) {
  return spawnSync(process.execPath, [h.entry, h.target, h.service, h.home], {
    encoding: 'utf8', timeout: 20000,
  });
}

async function preparations(h) {
  try {
    return (await readFile(h.log, 'utf8')).trim().split('\n').map(JSON.parse);
  } catch (error) {
    if (error.code === 'ENOENT') return [];
    throw error;
  }
}

test('installs all pinned packages into v3 and reuses the complete cache without fetching', async t => {
  const h = await workspace();
  t.after(() => rm(h.root, { recursive: true, force: true }));
  let result = await install(h);
  assert.equal(result.status, 0, result.stderr);
  const entry = join(h.target, 'v3/package/lib/index.js');
  assert.equal(result.stdout.trim(), entry);
  assert.equal((await preparations(h)).length, 1);
  for (const [name, release] of releases) {
    const manifest = JSON.parse(await readFile(join(h.target, 'v3',
      name === controllerName ? 'package/package.json' : join('node_modules', name, 'package.json'))));
    assert.equal(manifest.version, release.version);
  }
  const completed = (await stat(join(h.target, 'v3'))).mtimeMs;
  // A valid cache must not fetch, replace its generation, or prepare twice.
  await rename(archiveName(controllerName, version), archiveName(controllerName, version) + '.offline');
  try {
    result = await install(h);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    await rename(archiveName(controllerName, version) + '.offline', archiveName(controllerName, version));
  }
  assert.equal(result.stdout.trim(), entry);
  assert.equal((await preparations(h)).length, 2);
  assert.equal((await stat(join(h.target, 'v3'))).mtimeMs, completed);
  assert.deepEqual((await readdir(h.target)).filter(name => name.startsWith('.')), []);
});

test('replaces an incomplete generation and recovers with the real installer', async t => {
  const h = await workspace();
  t.after(() => rm(h.root, { recursive: true, force: true }));
  await mkdir(join(h.target, 'v3/package'), { recursive: true });
  await writeFile(join(h.target, 'v3/package/package.json'), JSON.stringify({ name: controllerName, version }));
  const result = await install(h);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), join(h.target, 'v3/package/lib/index.js'));
  assert.equal((await preparations(h)).length, 1);
  assert.deepEqual((await readdir(h.target)).filter(name => name.startsWith('.')), []);
});

test('a checksum failure leaves no generation, temporary archive, or profile preparation', async t => {
  const h = await workspace();
  t.after(() => rm(h.root, { recursive: true, force: true }));
  const path = archiveName(controllerName, version);
  const valid = await readFile(path);
  await writeFile(path, 'tampered archive');
  try {
    const result = await install(h);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /完整性校验失败/);
  } finally {
    await writeFile(path, valid);
  }
  assert.equal((await preparations(h)).length, 0);
  assert.deepEqual(await readdir(h.target), []);
  assert.deepEqual((await readdir(h.root)).filter(name => name.startsWith('.install-')), []);
});
