/** Mount upstream preference schemas in ACP's normal, editable profile tree. */
import { readFile as readPreferenceFile, mkdir as mkdirPreferenceDirectory } from 'node:fs/promises';
import { createRequire as preferenceRequire } from 'node:module';
import { join as preferencePath, dirname as preferenceDirectory } from 'node:path';
import { pathToFileURL as preferenceUrl } from 'node:url';

const preferenceBundleName = '@kense-labs/kwork-ui-preferences';
const preferenceFields = {
  'ui-theme': ['preference', 'fontSize', 'codeFontSize', 'terminalFontSize', 'textFontFamily', 'codeFontFamily', 'terminalFontFamily'],
  'ui-settings': ['enabled'],
};
const preferencePackages = { 'ui-theme': '@deepseek-ai/dsh-client-ui-theme', 'ui-settings': '@deepseek-ai/dsh-client-ui-settings' };

export async function prepareClientPreferences(service) {
  const require = preferenceRequire(preferencePath(service, 'apps/cli/package.json'));
  const bootPath = require.resolve('@deepseek-ai/dsh-app-boot');
  const boot = await import(preferenceUrl(bootPath).href);
  const bootRequire = preferenceRequire(bootPath);
  const { withFileLock, writeFileAtomic } = await import(preferenceUrl(bootRequire.resolve('@deepseek-ai/dsh-atomic-write')).href);
  const anchor = preferencePath(service, 'apps/cli/package.json');
  const profile = boot.loadProfile('dsh', 'acp', anchor);
  const manifestPath = preferencePath(profile.dir, 'package.json');
  await withFileLock(manifestPath, async () => {
    const current = boot.loadProfileDirectory('dsh', profile.dir, anchor);
    const flatten = entries => entries.flatMap(entry => [entry, ...entry.group && Array.isArray(entry.config) ? flatten(entry.config) : []]);
    const otherLayers = current.layers.filter(layer => layer.packageName !== preferenceBundleName);
    const existingEntries = flatten(boot.composeEntries([...otherLayers.map(layer => layer.patches), current.patches]));
    const existing = new Set(existingEntries.map(entry => entry.id));
    const officialHmr = existingEntries.some(entry => entry.id === 'hmr' && entry.name === '@deepseek-ai/dsh-hmr');
    const wanted = Object.keys(preferenceFields).filter(id => !existing.has(id));
    const manifest = boot.readProfileManifest('dsh', profile.dir);
    const bundles = manifest.dsh?.profile?.bundles ?? [];
    if (!wanted.length && !officialHmr && !bundles.includes(preferenceBundleName)) return;
    const directory = preferencePath(profile.dir, 'node_modules', preferenceBundleName);
    const packagePath = preferencePath(directory, 'package.json');
    try {
      const installed = JSON.parse(await readPreferenceFile(packagePath, 'utf8'));
      if (installed.kworkOwned !== 'client-preferences-v1') throw new Error('Client preference package path belongs to another installation');
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    const manager = preferenceRequire(require.resolve('@deepseek-ai/dsh-plugin-manager'));
    const schemaPath = manager.resolve('@deepseek-ai/schemastery/package.json');
    const schemaManifest = JSON.parse(await readPreferenceFile(schemaPath, 'utf8'));
    const schema = preferenceUrl(preferencePath(preferenceDirectory(schemaPath), schemaManifest.exports['.'].import)).href;
    const modules = typeof bundledPreferenceModules === 'undefined' ? {
      theme: await readPreferenceFile(new URL('./vendor/dsh-ui-preferences/theme.mjs', import.meta.url), 'utf8'),
      developer: await readPreferenceFile(new URL('./vendor/dsh-ui-preferences/developer.mjs', import.meta.url), 'utf8'),
      license: await readPreferenceFile(new URL('./vendor/dsh-ui-preferences/LICENSE', import.meta.url), 'utf8'),
    } : bundledPreferenceModules;
    await mkdirPreferenceDirectory(directory, { recursive: true });
    for (const [id, name] of Object.entries(preferencePackages)) {
      const packageDirectory = preferencePath(directory, 'node_modules', name);
      const source = modules[id === 'ui-theme' ? 'theme' : 'developer'];
      await writeFileAtomic(preferencePath(packageDirectory, 'index.mjs'), source.replaceAll('"@deepseek-ai/schemastery"', JSON.stringify(schema)), { mode: 0o600 });
      await writeFileAtomic(preferencePath(packageDirectory, 'LICENSE'), modules.license, { mode: 0o600 });
      await writeFileAtomic(preferencePath(packageDirectory, 'package.json'), JSON.stringify({name,version:'0.2.2-rc.3',type:'module',license:'MIT',
        main:'./index.mjs',exports:{'.':'./index.mjs','./package.json':'./package.json'},
        description:id === 'ui-theme' ? 'Shared theme and typography preferences' : 'Shared developer-tool preferences'},null,2)+'\n',{mode:0o600});
    }
    await writeFileAtomic(packagePath, JSON.stringify({ name: preferenceBundleName, version: '0.1.0', private: true,
      type: 'module', kworkOwned: 'client-preferences-v1', dependencies:Object.fromEntries(wanted.map(id=>[preferencePackages[id],'0.2.2-rc.3'])),
      dsh: { bundle: { patch: './cordis.patch.yml' } } }, null, 2) + '\n', { mode: 0o600 });
    await writeFileAtomic(preferencePath(directory, 'cordis.patch.yml'), JSON.stringify([{ insert: wanted.map(id => ({
      id, name: preferencePackages[id],
    })) },
    // ACP disables the base profile watcher. Desktop settings need the same
    // cross-process reloads as the official client. User patches remain above
    // this layer, including an explicit opt-out or a different HMR owner.
    ...(officialHmr ? [{ id: 'hmr', name: '@deepseek-ai/dsh-hmr', disabled: false }] : []),
    ], null, 2) + '\n', { mode: 0o600 });
    if (!bundles.includes(preferenceBundleName)) {
      const next = { ...manifest, dsh: { ...manifest.dsh, profile: { ...manifest.dsh?.profile, bundles: [...bundles, preferenceBundleName] } } };
      await writeFileAtomic(manifestPath, JSON.stringify(next, null, 2) + '\n', { mode: 0o600 });
    }
  });
}

/** Each connection owns its queue; reads cannot race earlier accepted writes. */
export function createClientPreferences(ctx) {
  let tail = Promise.resolve();
  const read = () => {
    const settings = ctx.get('settings');
    const namespaces = (settings?.describe({ redactSecrets: true }) ?? []).flatMap(entry => {
      const fields = Object.hasOwn(preferenceFields, entry.ns) ? preferenceFields[entry.ns] : undefined;
      if (!fields) return [];
      return [{ ns: entry.ns, revision: entry.revision, value: Object.fromEntries(fields
        .filter(field => Object.hasOwn(entry.value, field)).map(field => [field, entry.value[field]])) }];
    });
    return { writable: settings?.writable === true, namespaces };
  };
  return request => {
    if (request?.operation === 'read') return tail.then(read);
    const { namespace, changes, revision } = request ?? {};
    const fields = typeof namespace === 'string' && Object.hasOwn(preferenceFields, namespace) ? preferenceFields[namespace] : undefined;
    if (request?.operation !== 'set' || !fields || !Number.isSafeInteger(revision) || revision < 0 ||
        !changes || typeof changes !== 'object' || Array.isArray(changes) || !Object.keys(changes).length ||
        Object.keys(changes).some(field => !fields.includes(field))) return Promise.reject(new Error('Invalid client preference request'));
    const snapshot = structuredClone(changes);
    const task = tail.then(async () => {
      const current = read();
      if (!current.writable || !current.namespaces.some(entry => entry.ns === namespace)) throw new Error('Client preferences are unavailable or read-only');
      await ctx.get('settings').mutate(namespace, Object.entries(snapshot).map(([field, value]) => ({ op: 'set', path: [field], value })), revision);
      return read();
    });
    tail = task.then(() => undefined, () => undefined);
    return task;
  };
}

/** Import explicit desktop choices once; an existing Host field always wins. */
export async function importClientPreferences(ctx, path, service) {
  if (!path) return;
  const require = preferenceRequire(preferencePath(service, 'apps/cli/package.json'));
  const bootRequire = preferenceRequire(require.resolve('@deepseek-ai/dsh-app-boot'));
  const { withFileLock, writeFileAtomic } = await import(preferenceUrl(bootRequire.resolve('@deepseek-ai/dsh-atomic-write')).href);
  const directory = preferencePath(ctx.get('profileContext').dir, 'node_modules', preferenceBundleName);
  const markerPath = preferencePath(directory, 'desktop-import.json');
  let inserted;
  try { inserted = new Set(JSON.parse(await readPreferenceFile(preferencePath(directory,'cordis.patch.yml'),'utf8'))[0].insert.map(row=>row.id)); }
  catch (error) { if (error.code === 'ENOENT') return; throw error; }
  const owned = new Set(ctx.get('configEditor').entries().filter(entry =>
    inserted.has(entry.options.id) && entry.options.name === preferencePackages[entry.options.id]).map(entry => entry.options.id));
  if (!owned.size) return;
  await withFileLock(markerPath, async () => {
    const readOptional = async file => {
      try { return JSON.parse(await readPreferenceFile(file, 'utf8')); }
      catch (error) { if (error.code === 'ENOENT') return {}; throw error; }
    };
    const imported = await readOptional(markerPath);
    const local = await readOptional(path);
    const theme = {};
    if (['dark', 'light', 'system'].includes(local.themeMode)) theme.preference = local.themeMode;
    if (Number.isInteger(local.contentFontDelta) && local.contentFontDelta >= -4 && local.contentFontDelta <= 8) theme.fontSize = 14 + local.contentFontDelta;
    for (const [field, maximum] of [['codeFontSize', 16], ['terminalFontSize', 20]]) {
      if (Number.isInteger(local[field]) && local[field] >= 10 && local[field] <= maximum) theme[field] = local[field];
    }
    for (const field of ['textFontFamily', 'codeFontFamily', 'terminalFontFamily']) {
      if (typeof local[field] === 'string') theme[field] = local[field];
    }
    let enabled = local.developerTools;
    if (typeof enabled !== 'boolean') enabled = await readOptional(preferencePath(preferenceDirectory(path), 'developer-tools.json'));
    const values = { 'ui-theme': theme, 'ui-settings': typeof enabled === 'boolean' ? { enabled } : {} };
    for (const namespace of owned) {
      if (imported[namespace] || !values[namespace]) continue;
      const settings = ctx.get('settings');
      const entry = settings.describe({ redactSecrets: true }).find(row => row.ns === namespace);
      if (!entry || !settings.writable) continue;
      const ops = Object.entries(values[namespace]).filter(([field]) => !Object.hasOwn(entry.user ?? {}, field))
        .map(([field, value]) => ({ op: 'set', path: [field], value }));
      if (ops.length) await settings.mutate(namespace, ops, entry.revision);
      imported[namespace] = true;
    }
    await writeFileAtomic(markerPath, JSON.stringify(imported) + '\n', { mode: 0o600 });
  });
}
