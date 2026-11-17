// Real ACP/registry lifecycle; the two small preset definitions are test fixtures,
// not substitutes for the four official product presets.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, mkdtemp, readFile, writeFile, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';
import { adaptPresetAcpEntry } from '../services/agent-preset-session.mjs';

const [runtimeArgument, mockArgument, outputArgument] = process.argv.slice(2);
const official = process.argv.includes('--official');
assert.ok(runtimeArgument && mockArgument && process.env.DSH_NODE,
  'Usage: DSH_NODE=<node> bun scripts/agent-preset-runtime-smoke.mjs <runtime> <mock-server.ts> [artifacts]');
const runtime = resolve(runtimeArgument);
const output = outputArgument ? resolve(outputArgument) : await mkdtemp(join(tmpdir(), 'kwork-preset-runtime-'));
const home = join(output, 'home'), workspace = join(output, 'workspace');
const owned = join(home, 'profiles/acp/.kwork-runtime/preset-fixture');
await mkdir(owned, { recursive: true }); await mkdir(workspace, { recursive: true });
const url = path => pathToFileURL(path).href;
const acp = join(runtime, 'packages/acp/acp/lib');
await writeFile(join(owned, 'package.json'), '{"type":"module"}\n');
if (!official) {
await writeFile(join(owned, 'session.mjs'), `
import { AcpSession as Original } from ${JSON.stringify(url(join(acp, 'session.js')))};
import { mountAcpMcpServers } from ${JSON.stringify(url(join(acp, 'mcp.js')))};
import { installModelSelection } from '@deepseek-ai/dsh-agent';
import { presetAcpSession } from ${JSON.stringify(new URL('../services/agent-preset-session.mjs', import.meta.url).href)};
export const AcpSession = presetAcpSession({ AcpSession: Original, installModelSelection, mountAcpMcpServers });
`);
await writeFile(join(owned, 'acp.mjs'), adaptPresetAcpEntry(await readFile(join(acp, 'index.js'), 'utf8'),
  url(join(acp, 'index.js')), url(join(owned, 'session.mjs'))));
}
await writeFile(join(owned, 'roster.mjs'), `
import { writeFile } from 'node:fs/promises';
import { scopeOf } from '@deepseek-ai/dsh-scope';
export const inject = ['agentPresets', 'tools'];
export async function apply(ctx) {
  for (const id of ${official ? '[]' : "['standard', 'minimal']"}) {
    const plugins = id === 'standard' ? [{ id: 'read-only', name: '@deepseek-ai/dsh-tool-fs',
      config: { read: true, readImage: false, write: false, edit: false, applyPatch: false } }] : [];
    const dispose = await ctx.agentPresets.register({ id, plugins });
    ctx.effect(() => dispose);
  }
  ctx.on('agent/created', async ({ agent, source }) => {
    const result = { sessionId: agent.id, source, header: agent.session.header.agentPreset,
      selected: ctx.agentPresets.presetForSession(agent.session),
      composed: ctx.agentPresets.composedPreset(agent.ctx),
      tools: ctx.tools.schemas(scopeOf(agent.ctx)).map(tool => tool.name),
      leakedServices: ctx.agentPresets.inspectCompositions(agent.ctx).flatMap(row => row.leakedServices) };
    await writeFile(${JSON.stringify(output)} + '/' + agent.id + '.json', JSON.stringify(result));
  });
}
`);
const { startMockLlmServer } = await import(url(resolve(mockArgument)));
const server = await startMockLlmServer({ sequence: ['success'], repeatLast: true });
const disabled = ['tool-plugin-manager', 'tool-working-directory', 'tool-bash', 'tool-pwsh', 'tool-jobs', 'tool-fs', 'tool-fs-search',
  'skill-filesystem', 'tool-skill', 'command-goal', 'tool-goal', 'plan-mode', 'agent-instructions',
  'compaction-basic', 'command-compact', 'tool-result-pruner', 'tool-subagent-control', 'tool-subagent',
  'tool-subagent-fork', 'tool-agent-team', 'workflow-ptc', 'tool-workflow', 'tool-todo', 'tool-web'];
await writeFile(join(home, 'profiles/acp/cordis.patch.yml'), JSON.stringify([
  ...official ? [] : disabled.map(id => ({ id, disabled: true })),
  { id: 'session-persistence-jsonl', config: { root: join(home, 'sessions'), compression: 'none' } },
  { id: 'llm-deepseek', config: { baseURL: server.baseURL + '/v1' } },
  ...official ? [{ insert: [{ id: 'preset-observer', name: url(join(owned, 'roster.mjs')) }] }] : [
  { insert: [{ id: 'agent-preset-registry', name: url(join(runtime, 'packages/preset/agent-preset-registry/lib/index.js')),
    config: { default: 'standard' } }, { id: 'preset-fixture', name: url(join(owned, 'roster.mjs')) }] },
  { id: 'acp', disabled: true },
  { insert: [{ id: 'kwork-acp-presets', name: url(join(owned, 'acp.mjs')),
    inject: ['acpAppStartup', 'agentPresets'], config: { provider: 'deepseek-official', model: 'deepseek-v4-flash' } }] }],
]));
const bridge = join(output, 'bridge.mjs');
await writeFile(bridge, `${await readFile(new URL('../services/session-read-status.mjs', import.meta.url), 'utf8')}
const bundledAskUser = {};
${await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8')}`
  .replace("'./agent-terminal-bridge.mjs'", JSON.stringify(new URL('../services/agent-terminal-bridge.mjs', import.meta.url).pathname)));
const environment = { ...process.env, DSH_HOME: home, KWORK_WORKSPACE: workspace,
  KWORK_WORKSPACE_PREFERENCES: join(output, 'preferences.json'), DEEPSEEK_API_KEY: 'synthetic-local-only',
  DEEPSEEK_BASE_URL: server.baseURL + '/v1', DSH_TELEMETRY_DISABLED: '1' };
for (const name of ['OPENAI_API_KEY', 'KWORK_TERMINAL_MODULE', 'KWORK_TERMINAL_RUNTIME', 'KWORK_SCHEDULE_MODULE',
  'KWORK_CLIENT_PREFERENCES_MODULE', 'KWORK_AGENT_PRESETS_MODULE']) delete environment[name];
if (official) {
  environment.KWORK_AGENT_PRESETS_MODULE = new URL('../services/agent-preset-profile.mjs', import.meta.url).pathname;
  if (process.env.KWORK_PRESET_TEST_CACHE) {
    await symlink(resolve(process.env.KWORK_PRESET_TEST_CACHE), join(home, 'profiles/acp/.kwork-runtime/agent-presets-packages'), 'dir');
  }
}
const child = spawn(process.env.DSH_NODE, [bridge, runtime, join(runtime, 'apps/cli/lib/profile-boot.js')],
  { cwd: workspace, env: environment, stdio: ['pipe', 'pipe', 'pipe'] });
const exited = once(child, 'exit');
let stderr = '', sequence = 1;
child.stderr.on('data', data => { stderr += data; });
const pending = new Map();
child.on('exit', code => {
  for (const task of pending.values()) { clearTimeout(task.timer); task.reject(Error('ACP exited: ' + code)); }
  pending.clear();
});
const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
lines.on('line', line => {
  const frame = JSON.parse(line);
  const task = pending.get(frame.id);
  if (task) {
    pending.delete(frame.id); clearTimeout(task.timer);
    if (frame.error) task.reject(Error(JSON.stringify(frame.error))); else task.resolve(frame.result);
  } else if (frame.id) child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: frame.id,
    error: { code: -32601, message: 'Not supported in fixture' } }) + '\n');
});
function request(method, params = {}) {
  const id = sequence++;
  const promise = new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(Error('Timed out: ' + method)); }, 40_000);
    pending.set(id, { resolve, reject, timer });
  });
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n'); return promise;
}
const presets = params => request('_dsh/gpui/agent-preset-settings', params);
const inspect = async id => JSON.parse(await readFile(join(output, id + '.json'), 'utf8'));
try {
  await request('initialize', { protocolVersion: 1, clientCapabilities: {} });
  const roster = await presets({ operation: 'list' });
  await writeFile(join(output, 'roster.json'), JSON.stringify(roster, null, 2));
  assert.equal(roster.presets.length, official ? 4 : 2); assert.ok(roster.presets.every(row => !row.broken), JSON.stringify(roster));
  const first = await request('session/new', { cwd: workspace, mcpServers: [] });
  const initial = await inspect(first.sessionId);
  assert.equal(initial.composed, 'standard'); assert.equal(initial.header, 'standard');
  assert.ok(initial.tools.includes('read')); assert.deepEqual(initial.leakedServices, []);
  await presets({ operation: 'default', id: 'minimal', revision: roster.revision });
  assert.equal((await presets({ operation: 'session', sessionId: first.sessionId })).agentPreset, 'standard');
  const second = await request('session/new', { cwd: workspace, mcpServers: [] });
  const minimal = await inspect(second.sessionId);
  assert.equal(minimal.composed, 'minimal'); assert.deepEqual(minimal.tools.slice().sort(), official ? ['bash', 'read_image'] : []);
  const explicit = await request('session/new', { cwd: workspace, mcpServers: [], _meta: { 'kwork.agentPreset': 'standard' } });
  assert.equal((await inspect(explicit.sessionId)).composed, 'standard');
  await presets({ operation: 'select', id: 'minimal', sessionId: first.sessionId });
  await request('session/close', { sessionId: first.sessionId });
  await request('session/resume', { sessionId: first.sessionId, cwd: workspace, mcpServers: [] });
  const resumed = await inspect(first.sessionId);
  assert.equal(resumed.header, 'standard'); assert.equal(resumed.selected, 'minimal');
  assert.equal(resumed.composed, 'minimal'); assert.deepEqual(resumed.tools, minimal.tools);
  if (official) {
    for (const preset of ['ptc', 'cordis']) {
      const created = await request('session/new', { cwd: workspace, mcpServers: [], _meta: { 'kwork.agentPreset': preset } });
      const state = await inspect(created.sessionId);
      assert.equal(state.composed, preset); assert.ok(state.tools.includes('read')); assert.deepEqual(state.leakedServices, []);
      await request('session/close', { sessionId: created.sessionId });
    }
    const planning = await request('_dsh/gpui/session/plan-mode', { sessionId: explicit.sessionId, active: true });
    assert.equal(planning.active, true);
    await assert.rejects(request('_dsh/gpui/session/plan-mode', { sessionId: first.sessionId, active: true }), /不支持/);
    const schedule = operation => request('_dsh/gpui/schedule', {operation});
    const task = await schedule({
      action:'create', request:{
        title:'Preset fixture', prompt:'Reply OK.',
        timing:{kind:'at', at:new Date(Date.now() + 3_600_000).toISOString()},
        target:{kind:'new_once',create:{cwd:workspace,agentPreset:'minimal'}},
      },
    });
    assert.equal(task.target.create.agentPreset, 'minimal');
    const scheduledSession = task.target.sessionId;
    await schedule({action:'run',id:task.id});
    const scheduled = await inspect(scheduledSession);
    assert.equal(scheduled.header, 'minimal');
    assert.equal(scheduled.composed, 'minimal');
    assert.deepEqual(scheduled.tools, minimal.tools);
  }
  await request('session/prompt', { sessionId: first.sessionId, prompt: [{ type: 'text', text: 'Reply OK.' }] });
  await assert.rejects(presets({ operation: 'select', id: 'standard', sessionId: first.sessionId }), /already started|locked/);
  assert.equal((await presets({ operation: 'session', sessionId: first.sessionId })).canSelect, false);
  await assert.rejects(request('session/new', { cwd: workspace, mcpServers: [], _meta: { 'kwork.agentPreset': 'missing' } }));
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: true, official, initial, minimal, resumed,
    checks: ['pre-publication composition', 'default persistence', 'existing session unchanged', 'explicit selection',
      'scoped tools without leaks', 'selection projection survives close/resume', 'first turn locks selection', 'unknown preset refused'] }, null, 2));
  console.log(JSON.stringify({ passed: true, output }));
} finally {
  for (const task of pending.values()) clearTimeout(task.timer);
  child.kill('SIGTERM'); await exited; await server.close();
  await writeFile(join(output, 'stderr.log'), stderr);
}
