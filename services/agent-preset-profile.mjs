/** Install the official preset plane as an owned, atomic ACP profile bundle. */
import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readFile, realpath, lstat, readlink, rename, rmdir, symlink, unlink } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { resolvePresetPackages } from './agent-preset-packages.mjs';

const bundleName = '@kense-labs/kwork-agent-presets';
const acpName = '@kense-labs/kwork-acp';
const marker = 'agent-presets-v1';
const hash = value => createHash('sha256').update(value).digest('hex');
const asUrl = path => pathToFileURL(path).href;
const onWindows = process.platform === 'win32';
const linkDirectory = (source, link) => symlink(source, link, onWindows ? 'junction' : 'dir');
const removeDirectoryLink = onWindows ? rmdir : unlink;

async function publishGeneration(link, generation, ownedRoot) {
  let previous;
  try {
    if (!(await lstat(link)).isSymbolicLink()) throw Error('Agent 预设包路径已被其他配置使用，未覆盖。');
    previous = resolve(dirname(link), await readlink(link));
    const inside = relative(ownedRoot, previous);
    if (!inside || inside === '..' || inside.startsWith('..' + sep) || isAbsolute(inside)) {
      throw Error('Agent 预设包路径已被其他配置使用，未覆盖。');
    }
    if (relative(generation, previous) === '') return;
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  const temporary = link + '.' + randomUUID();
  await linkDirectory(generation, temporary);
  try {
    if (onWindows && previous) {
      // Win32 rename cannot replace a junction. Retain the old link until the
      // staged generation has been published under the profile write lock.
      const backup = temporary + '.previous';
      await rename(link, backup);
      try { await rename(temporary, link); }
      catch (error) { await rename(backup, link); throw error; }
      await removeDirectoryLink(backup);
    } else await rename(temporary, link);
  } finally {
    await removeDirectoryLink(temporary).catch(error => { if (error.code !== 'ENOENT') throw error; });
  }
}

async function assets() {
  if (typeof bundledPresetAssets !== 'undefined') return bundledPresetAssets;
  const read = path => readFile(new URL(path, import.meta.url), 'utf8');
  return {
    lock: JSON.parse(await read('./vendor/dsh-agent-presets/packages.json')),
    acp: await read('./vendor/dsh-agent-presets/acp.mjs'),
    license: await read('./vendor/dsh-agent-presets/LICENSE'),
    presets: Object.fromEntries(await Promise.all(['standard', 'ptc', 'minimal', 'cordis'].map(async id =>
      [id, await read(`./vendor/dsh-agent-presets/${id}.patch.yml`)]))),
  };
}

const modelRows = ['tool-plugin-manager', 'tool-bash', 'tool-pwsh', 'tool-jobs', 'tool-fs', 'tool-fs-search',
  'skill-filesystem', 'tool-skill', 'command-goal', 'tool-goal', 'plan-mode', 'agent-instructions',
  'compaction-basic', 'command-compact', 'tool-result-pruner', 'tool-subagent-control', 'tool-subagent',
  'tool-subagent-fork', 'workflow-ptc', 'tool-workflow', 'tool-todo', 'tool-web',
  // These two are extra model-facing rows in the companion ACP distribution.
  'tool-working-directory', 'tool-agent-team'];

export async function prepareAgentPresetProfile({ service, home, terminalRuntime }) {
  const packaged = await assets();
  const anchor = join(service, 'apps/cli/package.json');
  const require = createRequire(anchor);
  const bootPath = require.resolve('@deepseek-ai/dsh-app-boot');
  const boot = await import(asUrl(bootPath));
  if (boot.getDshRuntimeVersion() !== packaged.lock.version) {
    throw Error(`Agent 预设需要配套 Host ${packaged.lock.version}，当前为 ${boot.getDshRuntimeVersion()}。`);
  }
  const bootRequire = createRequire(bootPath);
  const { withFileLock, writeFileAtomic } = await import(asUrl(bootRequire.resolve('@deepseek-ai/dsh-atomic-write')));
  const profile = boot.loadProfile('dsh', 'acp', anchor);
  const ownedRoot = join(profile.dir, '.kwork-runtime/agent-presets-profile');
  const runtime = service;
  const roots = [...new Set([
    ...Object.values(packaged.presets).flatMap(text => [...text.matchAll(/name: '(@[^']+)'/g)]
      .map(match => match[1].split('/').slice(0, 2).join('/'))),
    '@deepseek-ai/dsh-agent-preset-registry', '@deepseek-ai/dsh-acp', '@deepseek-ai/dsh-schedule',
    '@deepseek-ai/dsh-cordis-host-runner', '@deepseek-ai/dsh-office-kit', '@deepseek-ai/dsh-skill-office',
  ])];
  const { sources, missing } = await resolvePresetPackages({ service, runtime, terminalRuntime, roots });
  if (missing.length) throw Error('Agent 预设缺少运行时依赖：' + missing.map(row => row.name).join(', '));
  const acp = sources.get('@deepseek-ai/dsh-acp');
  const manifestPath = join(profile.dir, 'package.json');
  return withFileLock(manifestPath, async () => {
    const current = boot.loadProfileDirectory('dsh', profile.dir, anchor);
    const layers = current.layers.filter(layer => layer.packageName !== bundleName);
    const flatten = rows => rows.flatMap(row => [row, ...row.group && Array.isArray(row.config) ? flatten(row.config) : []]);
    const existingRows = flatten(boot.composeEntries([...layers.map(layer => layer.patches), current.patches]));
    const ids = new Set(existingRows.map(row => row.id));
    const entries = [
      { id: 'agent-preset-registry', name: '@deepseek-ai/dsh-agent-preset-registry', config: { default: 'standard' } },
      { id: 'schedule', name: '@deepseek-ai/dsh-schedule' },
      { id: 'cordis-host-runner', name: '@deepseek-ai/dsh-cordis-host-runner' },
      { id: 'cordis-inspect-providers', name: '@deepseek-ai/dsh-tool-cordis/host' },
      { id: 'office-kit', name: '@deepseek-ai/dsh-office-kit', config: { outputDirectory: join(home, 'office-kit/outputs') } },
      { id: 'skill-office', name: '@deepseek-ai/dsh-skill-office', config: { pythonEnvironment: 'user' } },
    ].filter(row => !ids.has(row.id));
    const patches = [
      ...modelRows.filter(id => ids.has(id)).map(id => ({ id, disabled: true })),
      { insert: entries },
      { id: 'acp', disabled: true },
      { insert: [{ id: 'kwork-acp', name: acpName,
        inject: [...new Set([...(existingRows.find(row => row.id === 'acp')?.inject ?? []), 'agentPresets'])],
        config: existingRows.find(row => row.id === 'acp')?.config ?? {} }] },
    ];
    // A user-defined row wins. Preserve every expression in the shipped YAML.
    const presetText = Object.entries(packaged.presets).filter(([id]) => !ids.has('preset-' + id)).map(([, text]) => text).join('\n');
    const patchText = patches.map(row => '- ' + JSON.stringify(row)).join('\n') + '\n' + presetText;
    const bindings = [...sources].filter(([name, value]) => roots.includes(name) && name === value.manifest.name);
    const generation = join(ownedRoot, hash(JSON.stringify({ service, runtime, patchText, acp: packaged.acp,
      bindings: bindings.map(([name, value]) => [name, value.dir]) })));
    const replacement = join(generation, 'node_modules', acpName);
    await mkdir(generation, { recursive: true });
    for (const [name, source] of bindings) {
      const link = join(generation, 'node_modules', name);
      await mkdir(dirname(link), { recursive: true });
      try { await linkDirectory(source.dir, link); }
      catch (error) {
        if (error.code !== 'EEXIST' || relative(await realpath(link), source.dir) !== '') throw error;
      }
    }
    await writeFileAtomic(join(replacement, 'index.mjs'), packaged.acp, { mode: 0o600 });
    await writeFileAtomic(join(replacement, 'package.json'), JSON.stringify({ name: acpName, version: '0.1.0',
      type: 'module', license: 'MIT', description: 'KWork ACP sessions and preset composition',
      dependencies: acp.manifest.dependencies, peerDependencies: acp.manifest.peerDependencies,
      engines: { dsh: packaged.lock.version },
      main: './index.mjs', exports: { '.': './index.mjs', './package.json': './package.json' }, kworkOwned: marker }), { mode: 0o600 });
    await writeFileAtomic(join(replacement, 'LICENSE'), packaged.license, { mode: 0o600 });
    await writeFileAtomic(join(generation, 'cordis.patch.yml'), patchText, { mode: 0o600 });
    await writeFileAtomic(join(generation, 'package.json'), JSON.stringify({ name: bundleName, version: '0.1.0',
      private: true, type: 'module', kworkOwned: marker,
      dependencies: { ...Object.fromEntries(bindings.map(([name, source]) => [name, source.manifest.version])), [acpName]: '0.1.0' },
      dsh: { bundle: { patch: './cordis.patch.yml' } } }), { mode: 0o600 });
    const link = join(profile.dir, 'node_modules', bundleName);
    await mkdir(dirname(link), { recursive: true });
    await publishGeneration(link, generation, ownedRoot);
    const manifest = boot.readProfileManifest('dsh', profile.dir);
    const bundles = manifest.dsh?.profile?.bundles ?? [];
    if (!bundles.includes(bundleName)) {
      await writeFileAtomic(manifestPath, JSON.stringify({ ...manifest, dsh: { ...manifest.dsh,
        profile: { ...manifest.dsh?.profile, bundles: [...bundles, bundleName] } } }, null, 2) + '\n', { mode: 0o600 });
    }
    return { runtime, generation, packages: bindings.length };
  });
}
