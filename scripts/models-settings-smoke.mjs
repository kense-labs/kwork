// Exercise the official model settings store wire contract against a real,
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
  'Usage: DSH_NODE=<node> bun scripts/models-settings-smoke.mjs <runtime> <official mock-server.ts> [artifacts]');
const runtime = resolve(runtimeArgument);
const { startMockLlmServer } = await import(pathToFileURL(resolve(mockArgument)).href);
const directory = directoryArgument ? resolve(directoryArgument) : await mkdtemp(join(tmpdir(), 'kwork-models-settings-'));
const home = join(directory, 'home');
const workspace = join(directory, 'workspace');
await mkdir(join(home, 'profiles/acp'), { recursive: true });
await mkdir(workspace, { recursive: true });
await writeFile(join(workspace, 'live-result.json'), '{"nested":[false,null,7],"source":"real executor"}\n');
const server = await startMockLlmServer({ sequence: ['tool_call_success', 'success'], repeatLast: true,
  toolName: 'read', toolArguments: JSON.stringify({ file_path: join(workspace, 'live-result.json') }),
  successText: 'Live result verified.' });
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
const frames = [];
const trajectory = [];
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
  } else if (frame.method === '_dsh/gpui/tool-result') frames.push(frame.params);
  else if (frame.method === 'session/update' && frame.params.update?.sessionUpdate === 'gpui_trajectory_event') {
    trajectory.push(frame.params.update.event);
  }
  else if (frame.id) {
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
const remote = (method, args = []) => request('_dsh/gpui/providers/remote', { method, args });
const value = async (method, args = []) => {
  const reply = await remote(method, args);
  assert.equal(reply.ok, true, JSON.stringify(reply));
  return reply.value;
};
try {
  await request('initialize', { protocolVersion: 1, clientCapabilities: {} });
  const providers = await value('llm/listConfigurableProviders');
  const registered = await value('llm/listProviders');
  const described = await value('settings/describe');
  assert.ok(described.writable);
  const namespace = described.namespaces.find(row => row.ns === 'llm-pi-ai');
  assert.ok(namespace, 'Installed compatible provider schema is available');
  assert.ok(providers.some(row => row.provider === 'deepseek-official'));
  await writeFile(join(directory, 'fixture.json'), JSON.stringify({ directory: providers, registered, described }, null, 2));
  const ref = 'PARITY_MODELS_API_KEY';
  const secret = 'synthetic-model-secret';
  await value('credentials/set', [ref, secret]);
  const credential = await value('credentials/describe', [[ref]]);
  assert.equal(credential[ref].configured, true);
  assert.equal(JSON.stringify(credential).includes(secret), false);
  const profile = { displayName: 'Parity Gateway', api: 'openai-completions', baseURL: server.baseURL + '/v1',
    apiKeyEnv: ref, models: [{ id: 'fixture-model', name: 'Fixture model', contextWindow: 32000, maxTokens: 8192, input: ['text'] }] };
  const current = (await value('settings/describe')).namespaces.find(row => row.ns === 'llm-pi-ai');
  const written = await value('settings/mutate', ['llm-pi-ai', [{ op: 'set', path: ['providers', 'parity-models'], value: profile }], current.revision]);
  assert.deepEqual(written.user.providers['parity-models'], profile);
  assert.ok((await value('llm/listConfigurableProviders')).some(row => row.provider === 'parity-models' && row.declared));
  const stale = await remote('settings/mutate', ['llm-pi-ai', [{ op: 'unset', path: ['providers', 'parity-models'] }], current.revision]);
  assert.equal(stale.error?.code, 'settings/conflict');
  assert.ok((await value('settings/describe')).namespaces.find(row => row.ns === 'llm-pi-ai').user.providers['parity-models']);
  const models = await value('llm/discoverModels', ['llm-pi-ai', { provider: 'openai' }]);
  assert.ok(models.length > 0);
  await value('credentials/unset', [ref]);
  assert.equal((await value('credentials/describe', [[ref]]))[ref].configured, false);
  await value('settings/mutate', ['llm-pi-ai', [{ op: 'unset', path: ['providers', 'parity-models'] }]]);
  assert.equal((await value('settings/describe')).namespaces.find(row => row.ns === 'llm-pi-ai').user.providers?.['parity-models'], undefined);
  for (const [method, args] of [['credentials/describe', [['INVALID-REF']]], ['settings/mutate', ['permissions', []]], ['unknown', []]]) {
    assert.equal((await remote(method, args)).error?.code, 'gateway/bad-request');
  }
  await writeFile(join(directory, 'result.json'), JSON.stringify({ passed: true, providers: providers.length,
    catalogModels: models.length, checks: ['redacted views', 'credential roundtrip', 'path mutation', 'directory refresh', 'stale revision refused', 'catalog discovery', 'delete', 'invalid operations refused'] }, null, 2));
  console.log(JSON.stringify({ passed: true, directory }));
} finally {
  for (const task of pending.values()) clearTimeout(task.timer);
  child.kill('SIGTERM'); await exited; await server.close();
  await writeFile(join(directory, 'stderr.log'), stderr);
}
