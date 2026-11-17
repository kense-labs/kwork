import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { normalizeFontFamily } from '../ui/vendor/harness-settings-theme/font-size-specs.ts';

const source = await readFile(new URL('../ui/src/bridge.rs', import.meta.url), 'utf8');
const js = source.slice(source.indexOf('let workspacePreferencesWrite'), source.indexOf('export function dshListen(')).replaceAll('export ', '');
const create = new Function('window', js + '; return { invoke: dshInvoke, recover: dshRecoverFontPreferences };');

test('font recovery waits for queued saves and drops snapshots superseded by a new choice', async () => {
  const pending = [];
  const bridge = create({ __TAURI__: { core: { invoke: (command, args) => new Promise((resolve,reject) => pending.push({command,args,resolve,reject})) } } });
  const first = bridge.invoke('save_workspace_preferences',{preferences:{codeFontSize:14}});
  const failure = assert.rejects(first,/refused/);
  await Promise.resolve(); pending.shift().reject(Error('refused'));
  await failure;
  const recovery = bridge.recover(); await Promise.resolve();
  assert.equal(pending[0].command,'load_workspace_preferences');
  const later = bridge.invoke('save_workspace_preferences',{preferences:{codeFontSize:16}});
  pending.shift().resolve({codeFontSize:11});
  assert.equal(await recovery,null);
  await Promise.resolve(); pending.shift().resolve(null); await later;
  const settled = bridge.recover(); await Promise.resolve();
  pending.shift().resolve({codeFontSize:16});
  assert.deepEqual(await settled,{codeFontSize:16});
});

test('official family normalization preserves fallbacks and is idempotent', () => {
  const expected='"Arial", "Courier New", ui-monospace, "bad"';
  assert.equal(normalizeFontFamily(`  'Arial' , "Courier   New", ui-monospace,, <bad>\\\n`),expected);
  assert.equal(normalizeFontFamily(expected),expected);
  assert.equal(normalizeFontFamily('\ufeffArial\ufeff'), '"Arial"');
});
