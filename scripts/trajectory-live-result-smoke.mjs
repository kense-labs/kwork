// Real ACP execution against the official local Messages fixture. No provider
// credentials, synthetic session events, or replacement tool executor are used.
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
  'Usage: DSH_NODE=<node> bun scripts/trajectory-live-result-smoke.mjs <runtime> <official mock-server.ts> [artifacts]');
const runtime = resolve(runtimeArgument);
const { startMockLlmServer } = await import(pathToFileURL(resolve(mockArgument)).href);
const directory = directoryArgument ? resolve(directoryArgument) : await mkdtemp(join(tmpdir(), 'kwork-live-result-'));
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
async function resultFrame(token) {
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    const frame = frames.findLast(frame => frame.token === token);
    if (frame) return frame;
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  throw new Error('Missing result stream: ' + token);
}
try {
  await request('initialize', { protocolVersion: 1, clientCapabilities: {} });
  assert.deepEqual(await request('_dsh/gpui/settings-document', { operation: 'describe' }), { available: true });
  const document = await request('_dsh/gpui/settings-document', { operation: 'open', path: '/ignored.yml' });
  assert.equal(document.path, join(home, 'profiles/acp/cordis.patch.yml'), 'Settings document belongs to the isolated ACP Host');
  assert.ok((await readFile(document.path, 'utf8')).includes('session-persistence-jsonl'));
  await writeFile(join(directory, 'settings-document.json'), JSON.stringify({ available: true, document }, null, 2));
  const { sessionId } = await request('session/new', { cwd: workspace, mcpServers: [] });
  const resolvedDocument = await request('_dsh/gpui/session/resolve-document', { sessionId, path: 'live-result.json' });
  assert.deepEqual(resolvedDocument, { path: join(workspace, 'live-result.json'), cwd: workspace });
  assert.deepEqual(await request('_dsh/gpui/session/resolve-document', { sessionId, path: join(directory, 'outside.txt') }), { path: join(directory, 'outside.txt'), cwd: workspace });
  await assert.rejects(request('_dsh/gpui/session/resolve-document', { sessionId: 'missing-session', path: 'file.txt' }), /session\/not-found/);
  await writeFile(join(directory, 'document-path.json'), JSON.stringify(resolvedDocument, null, 2));
  await request('session/prompt', { sessionId, prompt: [{ type: 'text', text: 'Run the local result verification.' }] });
  await request('_dsh/gpui/session/flush', { sessionId });
  const histories = (await readdir(home, { recursive: true })).filter(file => file.endsWith('.jsonl'));
  const events = (await Promise.all(histories.map(file => readFile(join(home, file), 'utf8'))))
    .flatMap(source => source.trim().split('\n').filter(Boolean).map(line => JSON.parse(line)));
  const result = events.find(event => event.type === 'tool/result');
  assert.ok(result, 'The real Agent must commit a tool result');
  for (const type of ['user/message', 'system/message', 'tool/call', 'tool/result', 'assistant/message', 'request/header']) {
    const expected = events.filter(event => event.type === type).map(event => event.seq);
    assert.ok(expected.length > 0, 'The real fixture must exercise ' + type);
    assert.deepEqual(trajectory.filter(event => event.type === type).map(event => event.seq), expected,
      'Live ledger receives every durable ' + type + ' even when the transcript adapter returns early');
  }
  assert.equal(new Set(trajectory.map(event => event.seq)).size, trajectory.length, 'No duplicate ledger delivery');
  await request('_dsh/gpui/tool-result-watch', { sessionId, seq: result.seq, token: 'live', revision: 1 });
  const live = await resultFrame('live');
  assert.equal(live.available, true, 'Committed result is available from the live executor');
  assert.equal(live.value.path, join(workspace, 'live-result.json'));
  assert.equal(live.value.offset, 1);
  assert.deepEqual(live.value.lines[0], { number: 1, text: '{"nested":[false,null,7],"source":"real executor"}' });
  assert.notEqual(typeof live.value, 'string', 'The raw object must not be substituted with its rendered content');
  await request('_dsh/gpui/tool-result-watch', { sessionId, seq: result.seq + 10_000, token: 'missing', revision: 2 });
  assert.equal((await resultFrame('missing')).available, false);
  await request('_dsh/gpui/tool-result-watch', { sessionId: null, token: 'closed', revision: 3 });
  await writeFile(join(directory, 'result.json'), JSON.stringify({ sessionId, seq: result.seq, frames,
    trajectory: trajectory.map(({ type, seq }) => ({ type, seq })),
    modelRequests: server.requests.map(({ behavior, outcome }) => ({ behavior, outcome })) }, null, 2));
  console.log(JSON.stringify({ passed: true, directory, seq: result.seq, modelRequests: server.requests.length }));
} finally {
  for (const task of pending.values()) clearTimeout(task.timer);
  child.kill('SIGTERM'); await exited; await server.close();
  await writeFile(join(directory, 'stderr.log'), stderr);
}
