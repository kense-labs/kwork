import assert from 'node:assert/strict';
import test from 'node:test';
import SessionReadStatus from '../services/session-read-status.mjs';

function harness() {
  const listeners = new Set();
  const sessions = new Map();
  const published = [];
  const effects = [];
  const stored = new Set(['cold']);
  const ctx = {
    get(name) { return { sessions, sessionPersistence: { stat: async id => stored.has(id) ? {} : undefined } }[name]; },
    on(_name, listener) { listeners.add(listener); return () => listeners.delete(listener); },
    effect(setup) { effects.push(setup()); },
  };
  const status = new SessionReadStatus(ctx, state => published.push(state));
  return {
    status, published, effects, listeners,
    create(id) { sessions.set(id, { id }); return { id }; },
    end(session, turn, kind = 'completed') {
      for (const listener of listeners) listener(session, { type: 'turn/end', data: { turn, reason: { kind } } });
    },
  };
}

test('history is read until a live completion; failure follows the latest completion', () => {
  const h = harness();
  const session = h.create('history');
  assert.deepEqual(h.status.snapshot(), []);
  h.end(session, 1, 'error');
  h.end(session, 2);
  assert.deepEqual(h.status.snapshot(), [{ sessionId: 'history', completedTurn: 2, failed: false, readTurn: 0, manualUnread: false }]);
});

test('two frontends receive committed records; acknowledgement preserves manual unread', async () => {
  const h = harness();
  h.create('shared');
  const left = [];
  const right = [];
  h.published.push = (...items) => { for (const item of items) { left.push(item); right.push(item); } return items.length; };
  h.end({ id: 'shared' }, 1);
  await h.status.mark('shared', { type: 'read', throughTurn: 1 });
  await h.status.mark('shared', { type: 'unread' });
  h.end({ id: 'shared' }, 2);
  await h.status.mark('shared', { type: 'acknowledge', throughTurn: 1 });
  await h.status.mark('shared', { type: 'acknowledge', throughTurn: 9 });
  assert.deepEqual(left, right);
  assert.deepEqual(h.status.snapshot(), [{ sessionId: 'shared', completedTurn: 2, failed: false, readTurn: 2, manualUnread: true }]);
  await h.status.mark('shared', { type: 'read', throughTurn: 1 });
  assert.equal(h.status.snapshot()[0].manualUnread, false);
});

test('cold history can be marked without opening it, missing sessions are rejected', async () => {
  const h = harness();
  await h.status.mark('cold', { type: 'unread' });
  assert.equal(h.status.snapshot()[0].manualUnread, true);
  await assert.rejects(h.status.mark('missing', { type: 'unread' }), { code: 'session-status/not-found' });
});

test('wire actions are validated and disposal closes observation', async () => {
  const h = harness();
  h.create('s');
  await assert.rejects(h.status.mark('s', { type: 'read', throughTurn: -1 }), { code: 'session-status/invalid-action' });
  await assert.rejects(h.status.mark('s', { type: 'other' }), { code: 'session-status/invalid-action' });
  for (const dispose of h.effects) dispose();
  h.end({ id: 's' }, 1);
  assert.equal(h.listeners.size, 0);
  assert.deepEqual(h.status.snapshot(), []);
  await assert.rejects(h.status.mark('s', { type: 'unread' }), { code: 'session-status/unavailable' });
});
