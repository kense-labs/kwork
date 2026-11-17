// Exercise the official plugin inventory wire contract against a real,
// isolated ACP Host. Only a local mock server and synthetic credentials are used.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, mkdtemp, readFile, readdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';

const [runtimeArgument, mockArgument, directoryArgument] = process.argv.slice(2);
assert.ok(runtimeArgument && mockArgument && process.env.DSH_NODE,
  'Usage: DSH_NODE=<node> bun scripts/plugin-inventory-smoke.mjs <runtime> <official mock-server.ts> [artifacts]');
const runtime = resolve(runtimeArgument);
const { startMockLlmServer } = await import(pathToFileURL(resolve(mockArgument)).href);
const directory = directoryArgument ? resolve(directoryArgument) : await mkdtemp(join(tmpdir(), 'kwork-plugin-inventory-'));
const home = join(directory, 'home');
const workspace = join(directory, 'workspace');
await mkdir(join(home, 'profiles/acp'), { recursive: true });
await mkdir(workspace, { recursive: true });
const server = await startMockLlmServer({ sequence: ['success'], repeatLast: true });
await writeFile(join(home, 'profiles/acp/cordis.patch.yml'), JSON.stringify([
  { id: 'session-persistence-jsonl', config: { root: join(home, 'sessions'), compression: 'none' } },
  { id: 'llm-deepseek', config: { baseURL: server.baseURL + '/v1' } },
]));
const entry = join(directory, 'bridge.mjs');
await writeFile(entry, `${await readFile(new URL('../services/session-read-status.mjs', import.meta.url), 'utf8')}
const bundledAskUser = {};
${await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8')}`
  .replace("'./agent-terminal-bridge.mjs'", JSON.stringify(new URL('../services/agent-terminal-bridge.mjs', import.meta.url).pathname)));
const environment = { ...process.env, DSH_HOME: home, KWORK_WORKSPACE: workspace,
  KWORK_WORKSPACE_PREFERENCES: join(directory, 'preferences.json'), DEEPSEEK_API_KEY: 'synthetic-local-only',
  DEEPSEEK_BASE_URL: server.baseURL + '/v1', DSH_TELEMETRY_DISABLED: '1' };
delete environment.OPENAI_API_KEY;
const child = spawn(process.env.DSH_NODE, [entry, runtime, join(runtime, 'apps/cli/lib/profile-boot.js')],
  { cwd: workspace, env: environment, stdio: ['pipe', 'pipe', 'pipe'] });
const exited = once(child, 'exit');
let stderr = '';
child.stderr.on('data', data => { stderr += data; });
const pending = new Map();
let nextId = 1;
child.on('exit', code => {
  for (const task of pending.values()) {
    clearTimeout(task.timer); task.reject(new Error('ACP exited before responding: ' + code));
  }
  pending.clear();
});
const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
lines.on('line', line => {
  const frame = JSON.parse(line);
  if (frame.id && pending.has(frame.id)) {
    const task = pending.get(frame.id); pending.delete(frame.id); clearTimeout(task.timer);
    if (frame.error) task.reject(new Error(JSON.stringify(frame.error))); else task.resolve(frame.result);
  } else if (frame.id) {
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: frame.id, error: { code: -32601, message: 'Not supported in fixture' } }) + '\n');
  }
});
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
  const snapshot = await request('_dsh/gpui/plugin-inventory');
  assert.ok(snapshot.entries.length > 20, 'real ACP Loader inventory is populated');
  assert.ok(snapshot.entries.some(row => row.moduleName.includes('llm-deepseek')));
  for (const row of snapshot.entries) {
    assert.ok(row.entryId && row.moduleName);
    assert.equal(typeof row.enabled, 'boolean');
    assert.ok([null, 'pending', 'loading', 'active', 'failed', 'unloading'].includes(row.fiberPhase));
    assert.equal(row.config, undefined);
  }
  assert.equal(JSON.stringify(snapshot).includes('synthetic-local-only'), false);
  const repeated = await request('_dsh/gpui/plugin-inventory');
  assert.deepEqual(repeated.entries.map(row => row.entryId), snapshot.entries.map(row => row.entryId));
  await writeFile(join(directory, 'fixture.json'), JSON.stringify(snapshot, null, 2));
  await writeFile(join(directory, 'result.json'), JSON.stringify({ passed: true, entries: snapshot.entries.length,
    presets: snapshot.agentPresets?.length ?? 0, checks: ['real Loader inventory', 'live phases', 'no config or credentials', 'repeatable read'] }, null, 2));
  console.log(JSON.stringify({ passed: true, directory, entries: snapshot.entries.length }));
} finally {
  for (const task of pending.values()) clearTimeout(task.timer);
  child.kill('SIGTERM'); await exited; await server.close();
  await writeFile(join(directory, 'stderr.log'), stderr);
}
