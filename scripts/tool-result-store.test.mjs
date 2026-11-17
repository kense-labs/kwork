import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

// Execute the production retention/stream owner without booting an ACP runtime.
const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
const start = source.indexOf('function createToolResultStore(');
const end = source.indexOf('\nlet loadingMessageFeedback;', start);
assert(start >= 0 && end > start);
const create = new Function(`${source.slice(start, end)}\nreturn createToolResultStore;`)();

function fixture(limit = 1000) {
  const events = new Map();
  const sessions = new Map();
  const session = { id: 'session' }; sessions.set(session.id, session);
  const frames = [];
  let dispose;
  const ctx = {
    get(name) { assert.equal(name, 'sessions', 'inspection only reads the live registry'); return sessions; },
    on(name, callback) { events.set(name, callback); },
    effect(setup) { dispose = setup(); },
  };
  const store = create(ctx, frame => frames.push(structuredClone(frame)), limit);
  const value = (callId, value, options = {}) => events.get('tools/result')(
    { callId, agent: { session: options.session ?? session } }, { value, isError: options.error ?? false });
  const commit = (callId, seq, options = {}) => events.get('session/event')(options.session ?? session, {
    type: options.subcall ? 'tool/ptc-dispatch' : 'tool/result', seq,
    data: options.subcall ? { subCallId: callId } : { message: options.legacy ? { source: { callId } } : { toolCallId: callId } },
  });
  let revision = 0;
  const watch = (seq, token = `watch-${revision + 1}`) => store.watch({ sessionId: seq == null ? null : 'session', seq, token, revision: ++revision });
  return { store, session, sessions, frames, value, commit, watch, events, dispose: () => dispose() };
}

test('only a committed live result is published, including null and subcall values', () => {
  const h = fixture();
  h.watch(6);
  assert.equal(h.frames.at(-1).available, false);
  h.value('call', { nested: [null, false, 0], source: 'raw value' });
  assert.equal(h.frames.length, 1, 'uncommitted values have no durable result identity');
  h.commit('call', 6);
  assert.deepEqual(h.frames.at(-1).value, { nested: [null, false, 0], source: 'raw value' });
  assert.equal(h.frames.at(-1).update, 2);
  h.value('child', null); h.commit('child', 7, { subcall: true }); h.watch(7);
  assert.equal(h.frames.at(-1).available, true);
  assert.equal(h.frames.at(-1).value, null);
});

test('errors, undefined values and foreign Session instances never become raw results', () => {
  const h = fixture();
  h.value('error', { private: 'error' }, { error: true }); h.commit('error', 1); h.watch(1);
  assert.equal(h.frames.at(-1).available, false);
  h.value('missing', undefined); h.commit('missing', 2); h.watch(2);
  assert.equal(h.frames.at(-1).available, false);
  const foreign = { id: 'session' };
  h.value('foreign', { wrong: true }, { session: foreign }); h.commit('foreign', 3, { session: foreign }); h.watch(3);
  assert.equal(h.frames.at(-1).available, false);
});

test('retention counts pending values and watching does not refresh eviction order', () => {
  const h = fixture(2);
  h.value('one', 1); h.commit('one', 1);
  h.value('two', 2);
  h.watch(1); assert.equal(h.frames.at(-1).value, 1);
  h.value('three', 3);
  assert.equal(h.frames.at(-1).available, false, 'oldest committed value is evicted even while inspected');
  h.commit('two', 2, { legacy: true }); h.watch(2);
  assert.equal(h.frames.at(-1).value, 2, 'installed rc.3 message source is supported');
});

test('cold reads do not acquire a Session and replacements do not inherit its old values', () => {
  const h = fixture();
  h.sessions.clear(); h.watch(1);
  const coldFrames = h.frames.length;
  const next = { id: 'session' }; h.sessions.set('session', next);
  h.value('new', 'new instance', { session: next }); h.commit('new', 1, { session: next });
  assert.equal(h.frames.length, coldFrames, 'a cold stream stays bound to its empty lifetime');
  h.watch(1); assert.equal(h.frames.at(-1).value, 'new instance');
  h.events.get('session/disposed')(next);
  assert.equal(h.frames.at(-1).available, false);
  h.sessions.set('session', { id: 'session' }); h.watch(1);
  assert.equal(h.frames.at(-1).available, false);
});

test('replacement, stale requests, unsubscribe and runtime disposal release the selected stream', () => {
  const h = fixture();
  h.watch(1, 'old'); h.watch(2, 'current');
  assert.deepEqual(h.store.watch({ sessionId: 'session', seq: 1, token: 'late', revision: 1 }), { active: false });
  const count = h.frames.length;
  h.value('one', 1); h.commit('one', 1);
  assert.equal(h.frames.length, count);
  h.value('two', 2); h.commit('two', 2);
  assert.equal(h.frames.at(-1).token, 'current');
  h.watch(null); const closed = h.frames.length;
  h.events.get('session/disposed')(h.session);
  assert.equal(h.frames.length, closed);
  h.dispose(); h.watch(2);
  assert.equal(h.frames.length, closed);
});

test('invalid selection is rejected without releasing the current inspector', () => {
  const h = fixture(); h.watch(1, 'current');
  assert.throws(() => h.store.watch({ sessionId: 'session', seq: -1, token: 'invalid', revision: 2 }), /无效/);
  h.value('one', 1); h.commit('one', 1);
  assert.equal(h.frames.at(-1).token, 'current');
  assert.equal(h.frames.at(-1).value, 1);
});
