import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

async function harness({ origin = 'root', available = true, enabled = false } = {}) {
  const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
  const body = source.slice(source.indexOf('async function teamState'), source.indexOf('function observeSessionUpdates'));
  const factory = new Function('liveAgent', `class SessionOperationError extends Error {
    constructor(code, message = code) { super(message); this.code = code; }
  }\n${body}\nreturn teamState;`);
  const agent = { session: { id: 'root', header: { origin } } };
  const teamState = factory((ctx, id) => {
    assert.equal(id, 'root');
    return agent;
  });
  const saves = [];
  let state = { enabled, tasks: [] };
  let modelSelection = { lastUsed: null, next: null };
  const services = {
    agentTeams: {
      async setEnabled(target, value) {
        assert.equal(target, agent);
        saves.push(value);
        state = { ...state, enabled: value };
      },
    },
    sessionProjections: {
      snapshot(session, keys) {
        assert.equal(session, agent.session);
        assert.deepEqual(keys, ['agentTeam', 'modelSelection']);
        return { values: { agentTeam: state, modelSelection } };
      },
    },
  };
  const ctx = { get: (name) => available ? services[name] : undefined };
  return {
    ctx,
    agent,
    saves,
    setModelSelection(value) { modelSelection = value; },
    read(params = {}) {
      return teamState(ctx, { sessionId: 'root', request: 4, ...params });
    },
  };
}

test('Team reads use the actual root projection and distinguish model usage', async () => {
  const h = await harness({ enabled: true });
  assert.deepEqual(await h.read(), {
    available: true, request: 4,
    team: { enabled: true, tasks: [], hasUsedModel: false },
  });
  h.setModelSelection({ lastUsed: { provider: 'deepseek' }, next: null });
  assert.equal((await h.read()).team.hasUsedModel, true);
  h.setModelSelection(undefined);
  assert.equal((await h.read()).team.hasUsedModel, null);
  assert.deepEqual(h.saves, []);
});

test('Team saves delegate persistence to agentTeams and reread the projection', async () => {
  const h = await harness();
  assert.deepEqual(await h.read({ enabled: true }), {
    available: true, request: 4,
    team: { enabled: true, tasks: [], hasUsedModel: false },
  });
  assert.deepEqual(h.saves, [true]);
  await h.read({ enabled: false });
  assert.deepEqual(h.saves, [true, false]);
});

test('child sessions and unavailable Cordis services cannot expose or mutate Team', async () => {
  for (const h of [await harness({ origin: 'subagent' }), await harness({ available: false })]) {
    assert.deepEqual(await h.read(), { available: false, request: 4 });
    assert.deepEqual(await h.read({ enabled: true }), { available: false, request: 4 });
    assert.deepEqual(h.saves, []);
  }
});

test('invalid Team selections fail before entering the real service', async () => {
  const h = await harness();
  await assert.rejects(h.read({ enabled: 'yes' }), /智能体团队协作设置无效/);
  assert.deepEqual(h.saves, []);
});
