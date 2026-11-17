// Exercise the official client preference wire contract against a real,
// isolated ACP Host. Only a local mock server and synthetic credentials are used.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { parse } from 'yaml';
import { mkdir, mkdtemp, readFile, readdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';

const [runtimeArgument, mockArgument, directoryArgument] = process.argv.slice(2);
assert.ok(runtimeArgument && mockArgument && process.env.DSH_NODE,
  'Usage: DSH_NODE=<node> bun scripts/client-preferences-smoke.mjs <runtime> <official mock-server.ts> [artifacts]');
const runtime = resolve(runtimeArgument);
const { startMockLlmServer } = await import(pathToFileURL(resolve(mockArgument)).href);
const directory = directoryArgument ? resolve(directoryArgument) : await mkdtemp(join(tmpdir(), 'kwork-client-preferences-'));
const home = join(directory, 'home');
const workspace = join(directory, 'workspace');
await mkdir(join(home, 'profiles/acp'), { recursive: true });
await mkdir(workspace, { recursive: true });
const migration = process.env.KWORK_TEST_MIGRATION === '1';
if (migration) await writeFile(join(directory,'preferences.json'),JSON.stringify({themeMode:'dark',contentFontDelta:2,codeFontSize:12,terminalFontSize:14,textFontFamily:'Arial',developerTools:false}));
const server = await startMockLlmServer({ sequence: ['success'], repeatLast: true });
await writeFile(join(home, 'profiles/acp/cordis.patch.yml'), JSON.stringify([
  { id: 'session-persistence-jsonl', config: { root: join(home, 'sessions'), compression: 'none' } },
  { id: 'llm-deepseek', config: { baseURL: server.baseURL + '/v1' } },
  ...(migration ? [{id:'ui-theme',config:{preference:'light'}}] : []),
]));
const entry = join(directory, 'bridge.mjs');
await writeFile(entry, `${await readFile(new URL('../services/session-read-status.mjs', import.meta.url), 'utf8')}
const bundledAskUser = {};
${await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8')}`
  .replace("'./agent-terminal-bridge.mjs'", JSON.stringify(new URL('../services/agent-terminal-bridge.mjs', import.meta.url).pathname)));
const environment = { ...process.env, DSH_HOME: home, KWORK_WORKSPACE: workspace,
  KWORK_CLIENT_PREFERENCES_MODULE: resolve('services/client-preferences.mjs'), KWORK_WORKSPACE_PREFERENCES: join(directory, 'preferences.json'), DEEPSEEK_API_KEY: 'synthetic-local-only',
  DEEPSEEK_BASE_URL: server.baseURL + '/v1', DSH_TELEMETRY_DISABLED: '1' };
delete environment.OPENAI_API_KEY;
let child, exited;
const pending = new Map();
let changes=0, nextId=1, stderr='';
function start() {
child = spawn(process.env.DSH_NODE, [entry, runtime, join(runtime, 'apps/cli/lib/profile-boot.js')],
  { cwd: workspace, env: environment, stdio: ['pipe', 'pipe', 'pipe'] });
exited = once(child, 'exit');
child.stderr.on('data', data => { stderr += data; });
child.on('exit', code => {
  for (const task of pending.values()) {
    clearTimeout(task.timer); task.reject(new Error('ACP exited before responding: ' + code));
  }
  pending.clear();
});
const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
lines.on('line', line => {
  const frame = JSON.parse(line);
  if (frame.method === '_dsh/gpui/client-preferences-changed') changes++;
  if (frame.id && pending.has(frame.id)) {
    const task = pending.get(frame.id); pending.delete(frame.id); clearTimeout(task.timer);
    if (frame.error) task.reject(new Error(JSON.stringify(frame.error))); else task.resolve(frame.result);
  } else if (frame.id) {
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: frame.id, error: { code: -32601, message: 'Not supported in fixture' } }) + '\n');
  }
});
}
start();
function request(method, params = {}) {
  const id = nextId++;
  const result = new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('Timed out: ' + method)); }, 40_000);
    pending.set(id, { resolve, reject, timer });
  });
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  return result;
}
try {
  await request('initialize', { protocolVersion: 1, clientCapabilities: {} });
  const call = params => request('_dsh/gpui/client-preferences', params);
  const before = await call({ operation: 'read' });
  assert.equal(before.writable, true);
  const watcher = (await request('_dsh/gpui/plugin-inventory')).entries.find(row => row.moduleName === '@deepseek-ai/dsh-hmr');
  assert.equal(watcher?.fiberPhase, 'active', 'desktop enables the official profile watcher');
  const theme = before.namespaces.find(row => row.ns === 'ui-theme');
  const developer = before.namespaces.find(row => row.ns === 'ui-settings');
  assert.deepEqual(theme.value, migration ? {preference:'light',fontSize:16,codeFontSize:12,terminalFontSize:14,textFontFamily:'Arial',codeFontFamily:'',terminalFontFamily:''} : { preference: 'system', fontSize: 14, codeFontSize: 11, terminalFontSize: 13,
    textFontFamily: '', codeFontFamily: '', terminalFontFamily: '' });
  assert.equal(developer.value.enabled, !migration);
  const first = await call({ operation: 'set', namespace: 'ui-theme', revision: theme.revision,
    changes: { preference: 'dark', fontSize: 17, codeFontFamily: 'Menlo' } });
  assert.equal(first.namespaces.find(row => row.ns === 'ui-theme').value.fontSize, 17);
  await assert.rejects(call({ operation: 'set', namespace: 'ui-theme', revision: theme.revision, changes: { fontSize: 18 } }));
  await assert.rejects(call({ operation: 'set', namespace: 'ui-theme', revision: 1, changes: { apiKey: 'forbidden' } }));
  await assert.rejects(call({ operation: 'set', namespace: 'llm-deepseek', revision: 0, changes: { enabled: false } }));
  await assert.rejects(call({ operation: 'set', namespace: 'ui-theme', revision: first.namespaces.find(row=>row.ns==='ui-theme').revision, changes: { fontSize: 999 } }));
  let after = await call({ operation: 'set', namespace: 'ui-settings', revision: developer.revision, changes: { enabled: !developer.value.enabled } });
  assert.equal(after.namespaces.find(row => row.ns === 'ui-settings').value.enabled, !developer.value.enabled);
  const profile = await readFile(join(home, 'profiles/acp/cordis.patch.yml'), 'utf8');
  assert.ok(profile.includes('ui-theme') && profile.includes('ui-settings'));
  assert.ok(changes >= 2, 'Host broadcasts accepted preference updates');
  const beforeExternal = after;
  const changesBeforeExternal = changes;
  const external = parse(profile);
  external.find(row => row.id === 'ui-theme').config.fontSize = 18;
  external.find(row => row.id === 'ui-settings').config.enabled = developer.value.enabled;
  await writeFile(join(home, 'profiles/acp/cordis.patch.yml'), JSON.stringify(external));
  const reloadDeadline = Date.now() + 12_000;
  do {
    after = await call({ operation: 'read' });
    if (after.namespaces.find(row => row.ns === 'ui-theme').value.fontSize === 18 &&
        after.namespaces.find(row => row.ns === 'ui-settings').value.enabled === developer.value.enabled) break;
    await new Promise(resolve => setTimeout(resolve, 50));
  } while (Date.now() < reloadDeadline);
  assert.equal(after.namespaces.find(row => row.ns === 'ui-theme').value.fontSize, 18, 'external profile edit reloads without restart');
  assert.equal(after.namespaces.find(row => row.ns === 'ui-settings').value.enabled, developer.value.enabled);
  assert.ok(changes > changesBeforeExternal, 'external changes broadcast to desktop');
  await assert.rejects(call({ operation: 'set', namespace: 'ui-theme',
    revision: beforeExternal.namespaces.find(row => row.ns === 'ui-theme').revision, changes: { fontSize: 20 } }));
  child.kill('SIGTERM'); await exited;
  await writeFile(join(directory,'preferences.json'),JSON.stringify({themeMode:'light',contentFontDelta:8,developerTools:!after.namespaces.find(row=>row.ns==='ui-settings').value.enabled}));
  start();await request('initialize',{protocolVersion:1,clientCapabilities:{}});
  const restored=await call({operation:'read'});
  assert.deepEqual(restored.namespaces.map(row=>[row.ns,row.value]),after.namespaces.map(row=>[row.ns,row.value]));
  const manifest=JSON.parse(await readFile(join(home,'profiles/acp/package.json'),'utf8'));
  assert.equal(manifest.dsh.profile.bundles.filter(name=>name==='@kense-labs/kwork-ui-preferences').length,1);
  child.kill('SIGTERM');await exited;
  const custom=join(directory,'existing-theme.mjs');
  await writeFile(custom,await readFile(join(home,'profiles/acp/node_modules/@kense-labs/kwork-ui-preferences/node_modules/@deepseek-ai/dsh-client-ui-theme/index.mjs'),'utf8'));
  const ownPatch=join(home,'profiles/acp/cordis.patch.yml');
  const customValue={...after.namespaces.find(row=>row.ns==='ui-theme').value,fontSize:19};
  await writeFile(ownPatch,JSON.stringify([{insert:[{id:'ui-theme',name:pathToFileURL(custom).href,config:customValue}]},
    {id:'hmr',disabled:true},
    ...parse(await readFile(ownPatch,'utf8')).filter(row=>row.id!=='ui-theme')]));
  start();await request('initialize',{protocolVersion:1,clientCapabilities:{}});
  const preserved=await call({operation:'read'});
  const optedOut = (await request('_dsh/gpui/plugin-inventory')).entries.find(row => row.moduleName === '@deepseek-ai/dsh-hmr');
  assert.equal(optedOut?.enabled, false, 'explicit user watcher opt-out wins the desktop bundle');
  assert.deepEqual(preserved.namespaces.find(row=>row.ns==='ui-theme').value,customValue);
  assert.deepEqual(preserved.namespaces.find(row=>row.ns==='ui-settings').value,after.namespaces.find(row=>row.ns==='ui-settings').value);
  const owned=JSON.parse(await readFile(join(home,'profiles/acp/node_modules/@kense-labs/kwork-ui-preferences/cordis.patch.yml'),'utf8'));
  assert.equal(owned[0].insert.some(entry=>entry.id==='ui-theme'),false,'existing namespace owner is preserved');
  await writeFile(join(directory, 'result.json'), JSON.stringify({ passed: true, before, after, restored, preserved, migration, changes,
    checks: ['upstream default schemas mounted in actual ACP', 'accepted theme and developer writes', 'stale revision refusal',
      'schema and field whitelist refusal', 'durable profile', 'Host change events', 'external profile edits reload and broadcast', 'external edits invalidate stale revisions', 'cold restore ignores later installation cache', 'idempotent bundle registration', 'existing namespace owner preserved', 'explicit watcher opt-out preserved', ...(migration ? ['explicit local preference migration', 'explicit Host value wins migration'] : [])] }, null, 2));
  console.log(JSON.stringify({ passed: true, directory, changes }));
} finally {
  for (const task of pending.values()) clearTimeout(task.timer);
  child.kill('SIGTERM'); await exited; await server.close();
  await writeFile(join(directory, 'stderr.log'), stderr);
}
