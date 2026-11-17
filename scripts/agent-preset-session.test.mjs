import { test } from 'node:test';
import assert from 'node:assert/strict';
import { presetAcpSession, adaptPresetAcpEntry } from '../services/agent-preset-session.mjs';

function fixture() {
  const calls = [];
  let selected = 'minimal', defaultId = 'standard';
  let mounting = async () => {};
  const abort = new AbortController();
  const registry = {
    async resolve(id = defaultId) {
      if (!['standard', 'minimal', 'broken'].includes(id)) throw Error('unknown preset');
      return { id, ...(id === 'broken' ? { broken: 'broken composition' } : {}) };
    },
    presetForSession: () => selected,
    async mount(ctx, id) { calls.push(['mount', id, ctx]); await mounting(); },
  };
  const agentCtx = {}, agent = { session: { header: { agentPreset: 'standard' } } };
  const handle = { agent, dispose() {} };
  const ctx = { get: name => name === 'agentPresets' ? registry : undefined, agents: {
    async create(options) {
      calls.push(['create', options]); defaultId = 'minimal';
      await options.setup(agentCtx, agent); calls.push(['published']); return handle;
    },
    async resume(options) { calls.push(['resume', options]); await options.setup(agentCtx, agent); calls.push(['published']); return handle; },
  } };
  class Base { constructor(context, owned, notify) { this.context = context; this.owned = owned; this.notify = notify; } }
  const Session = presetAcpSession({ AcpSession: Base,
    installModelSelection: context => calls.push(['model', context]),
    mountAcpMcpServers: async (...args) => calls.push(['mcp', ...args]),
  });
  const options = { sessionId: 'fixture', cwd: '/fixture', mcpServers: [{ name: 'retained' }],
    agentOptions: { provider: 'fixture' }, signal: abort.signal, notify() {} };
  return { calls, ctx, options, Session, Base, handle, abort, agentCtx,
    select: id => { selected = id; }, mount: fn => { mounting = fn; } };
}

test('new ACP sessions pin the default in the header and mount it before publication/MCP', async () => {
  const f = fixture();
  const session = await f.Session.create(f.ctx, f.options);
  assert.ok(session instanceof f.Base);
  assert.equal(session.owned, f.handle); assert.equal(session.notify, f.options.notify);
  assert.deepEqual(f.calls.map(c => c[0]), ['create', 'model', 'mount', 'mcp', 'published']);
  assert.deepEqual(f.calls[0][1].meta, { cwd: '/fixture', agentPreset: 'standard' });
  assert.equal(f.calls[0][1].agentOptions, f.options.agentOptions);
  assert.equal(f.calls[2][1], 'standard');
  assert.deepEqual(f.calls[3], ['mcp', f.agentCtx, f.options.mcpServers, '/fixture']);
});

test('explicit presets override defaults; unknown/broken presets never create an Agent', async () => {
  for (const id of ['missing', 'broken', '', 42]) {
    const f = fixture();
    await assert.rejects(f.Session.create(f.ctx, { ...f.options, agentPreset: id }));
    assert.equal(f.calls.length, 0);
  }
  const f = fixture(); await f.Session.create(f.ctx, { ...f.options, agentPreset: 'minimal' });
  assert.equal(f.calls[0][1].meta.agentPreset, 'minimal');
});

test('resume uses the committed selection, retaining creation history', async () => {
  const f = fixture();
  await f.Session.resume(f.ctx, f.options);
  assert.deepEqual(f.calls.map(c => c[0]), ['resume', 'model', 'mount', 'mcp', 'published']);
  assert.equal(f.calls[2][1], 'minimal');
  assert.equal(f.handle.agent.session.header.agentPreset, 'standard');
  assert.equal(f.calls[0][1].resumeSessionId, 'fixture');
});

test('missing historical preset, failed mounts and cancellation do not publish or start MCP', async () => {
  for (const fail of ['missing', 'mount', 'abort']) {
    const f = fixture();
    if (fail === 'missing') f.select('removed');
    if (fail === 'mount') f.mount(async () => { throw Error('cannot mount'); });
    if (fail === 'abort') f.mount(async () => { f.abort.abort(Error('cancelled')); });
    await assert.rejects(f.Session.resume(f.ctx, f.options));
    assert.equal(f.calls.some(c => c[0] === 'published' || c[0] === 'mcp'), false);
  }
});

test('legacy histories use the default; a missing registry and pre-cancelled requests fail closed', async () => {
  const f = fixture(); f.select(null); await f.Session.resume(f.ctx, f.options);
  assert.equal(f.calls[2][1], 'standard');
  const cancelled = fixture(); cancelled.abort.abort();
  await assert.rejects(cancelled.Session.create(cancelled.ctx, cancelled.options));
  assert.equal(cancelled.calls.length, 0);
  await assert.rejects(cancelled.Session.create({ get: () => undefined }, cancelled.options), /尚未就绪/);
});

test('entry adaptation preserves module identities and rejects missing/duplicate ACP seams', () => {
  const source = `import { AcpSession } from "./session.js";
import { AcpMcpConfigError } from "./mcp.js";
const args = {sessionId, cwd: params.cwd, mcpServers: params.mcpServers,};
const host = {async create(sessionId, cwd) {
  await createSession(sessionId, { cwd, mcpServers: [] }, lifetime.signal, true);
}};`;
  const result = adaptPresetAcpEntry(source, 'file:///installed/lib/index.js', 'file:///owned/session.mjs');
  assert.match(result, /from "file:\/\/\/installed\/lib\/mcp.js"/);
  assert.match(result, /from "file:\/\/\/owned\/session.mjs"/);
  assert.match(result, /params\._meta\?\.\['kwork.agentPreset'\]/);
  assert.match(result, /async create\(sessionId, cwd, agentPreset\)/);
  assert.throws(() => adaptPresetAcpEntry(source.replace('async create', 'async createChanged'), '', ''), /不兼容/);
  assert.throws(() => adaptPresetAcpEntry(source + source, '', ''), /不兼容/);
});
