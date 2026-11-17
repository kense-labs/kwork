import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('goal edits use the displayed identity and revision even after goal replacement', async () => {
  const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
  const body = source.slice(source.indexOf('function currentGoalView'), source.indexOf('async function teamState'));
  const agent = { id: 'session' };
  const { goalAction, goalState } = new Function('liveAgent', `
    class SessionOperationError extends Error {
      constructor(code, message = code) { super(message); this.code = code; }
    }
    ${body}
    return { goalAction, goalState };
  `)((_, sessionId) => {
    assert.equal(sessionId, agent.id);
    return agent;
  });
  let goal = { id: 'replacement', revision: 1, objective: 'current' };
  const calls = [];
  const goals = {
    get: () => goal,
    edit(target, ref, { objective }) {
      assert.equal(target, agent);
      calls.push(ref);
      if (goal.id !== ref.id || goal.revision !== ref.revision) throw new Error('stale goal');
      goal = { ...goal, revision: goal.revision + 1, objective };
    },
  };
  const ctx = { get: key => key === 'goals' ? goals : undefined };
  const args = { sessionId: agent.id, action: 'edit', revision: 1, objective: '  第一行\n第二行  ' };
  assert.throws(() => goalAction(ctx, args), /缺少目标身份或版本/);
  assert.deepEqual(calls, []);
  assert.throws(() => goalAction(ctx, { ...args, goalId: 'replaced' }), /stale goal/);
  assert.equal(goal.objective, 'current');
  assert.deepEqual(calls, [{ id: 'replaced', revision: 1 }]);

  const firstRead = goalState(ctx, args);
  const saved = goalAction(ctx, { ...args, goalId: 'replacement' });
  assert.equal(saved.goal.objective, '第一行\n第二行');
  assert.equal(saved.goal.revision, 2);
  assert.ok(saved.sequence > firstRead.sequence);
});
