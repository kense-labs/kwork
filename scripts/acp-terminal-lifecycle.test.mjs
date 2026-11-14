import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');

// Load the production functions with their real module constants and stream
// implementation, replacing only the ACP boot and transport boundary.
async function harness(frames, { firstDone = false, retain } = {}) {
  const writes = [];
  const errors = [];
  const prefix = 'const terminalWatches = new Map();\n';
  const functions = source.slice(source.indexOf('class SessionOperationError'), source.indexOf('function terminalAttachmentKey'));
  const api = new Function('process', 'terminalInfo', `${prefix}${functions}\nreturn { watchTerminals, terminalWatches };`)(
    { stdout: { write: line => writes.push(JSON.parse(line)) }, stderr: { write: line => errors.push(line) } },
    info => ({ ...info, shell: info.shell?.name ?? info.shell }),
  );
  const streams = [];
  const retained = new Map();
  const terminals = {
    watch(_session, signal) {
      const stream = (async function * () {
        if (firstDone) return;
        yield frames;
        yield frames;
        await new Promise(resolve => {
          if (signal.aborted) resolve();
          else signal.addEventListener('abort', resolve, { once: true });
        });
      })();
      streams.push(stream);
      return stream;
    },
    retain(session, id, signal) {
      const state = { aborted: false };
      retained.set(id, state);
      return retain?.(session, id, signal, state) ?? (async function * () {
        yield { type: 'snapshot' };
        await new Promise(resolve => {
          if (signal.aborted) { state.aborted = true; resolve(); }
          else signal.addEventListener('abort', () => { state.aborted = true; resolve(); }, { once: true });
        });
      })();
    },
  };
  return { ...api, ctx: { get: () => terminals }, writes, errors, retained, streams };
}

test('watch publishes normalized rosters and retains only requested window tabs', async () => {
  const h = await harness([{ id: 'terminal', title: 'Agent', shell: { name: 'zsh' } }]);
  const first = await h.watchTerminals(h.ctx, { sessionId: 'session', request: 1 });
  assert.equal(first[0].shell, 'zsh');
  const retained = await h.watchTerminals(h.ctx, { sessionId: 'session', active: ['terminal', 'terminal', 4] });
  assert.equal(retained[0].id, 'terminal');
  assert(h.retained.has('terminal'));
  assert.equal(h.retained.size, 1);
  await h.watchTerminals(h.ctx, { sessionId: 'session', active: [] });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(h.retained.get('terminal').aborted, true);
  await h.watchTerminals(h.ctx, { sessionId: 'session', active: false });
  assert.equal(h.terminalWatches.size, 0);
});

test('a retain stream failure removes only that hold and can be retried', async () => {
  let attempts = 0;
  const h = await harness([], { retain: async function * () {
    attempts++;
    throw new Error('fixture retain disconnected');
  } });
  await h.watchTerminals(h.ctx, { sessionId: 'session', request: 1 });
  await h.watchTerminals(h.ctx, { sessionId: 'session', active: ['terminal'] });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(h.terminalWatches.get('session').retained.size, 0);
  await h.watchTerminals(h.ctx, { sessionId: 'session', active: ['terminal'] });
  assert.equal(attempts, 2);
  await h.watchTerminals(h.ctx, { sessionId: 'session', active: false });
});

test('watch initialization failure and replacement clean up their own stream and holds', async () => {
  const h = await harness([], { firstDone: true });
  await assert.rejects(h.watchTerminals(h.ctx, { sessionId: 'session', request: 1 }), /清单已关闭/);
  assert.equal(h.terminalWatches.has('session'), false, 'a failed initial roster must not leave a poisoned watch');
  const live = await harness([]);
  await live.watchTerminals(live.ctx, { sessionId: 'session', request: 1 });
  await live.watchTerminals(live.ctx, { sessionId: 'session', active: ['terminal'] });
  const previous = live.terminalWatches.get('session');
  await live.watchTerminals(live.ctx, { sessionId: 'session', request: 2 });
  assert.equal(previous.controller.signal.aborted, true);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(live.retained.get('terminal').aborted, true);
  await live.watchTerminals(live.ctx, { sessionId: 'session', active: false });
});

function inputSource(lines) {
  return source.slice(source.indexOf('const input ='), source.indexOf('const { resolveDshHome }'))
    .replace("createInterface({ input: process.stdin, crlfDelay: Infinity })", 'fixtureInput')
    .replace('const acpInput = new PassThrough();', '')
    + source.slice(source.indexOf('async function forward()'), source.indexOf('try {\n  // Start consuming'));
}

test('stdin FIFO preserves multiple pre-ready lines and forwards EOF exactly once', async () => {
  const lines = ['initialize', 'session/new', 'second-early-frame'];
  const fixtureInput = {
    handlers: {},
    on(name, handler) { this.handlers[name] = handler; },
    once(name, handler) { this.handlers[name] = handler; },
    emit(name, value) { this.handlers[name]?.(value); },
  };
  const acpInput = { writes: [], write(line) { this.writes.push(line); return true; }, end() { this.ended = true; } };
  const extensionMethods = ['_dsh/gpui/terminal-watch'];
  const api = new Function('fixtureInput', 'acpInput', 'once', 'extensionMethods',
    `${inputSource(lines)}\nfor (const line of ${JSON.stringify(lines)}) input.emit('line', line);
input.emit('close');
return forward();`)(fixtureInput, acpInput, async () => {}, extensionMethods);
  await api;
  assert.deepEqual(acpInput.writes, lines.map(line => line + '\n'));
  assert.equal(acpInput.ended, true);
});

test('stdin EOF and profile rejection both terminate the ACP stream without a hanging wake', async () => {
  const fixtureInput = { on() {}, once(_name, handler) { handler(); } };
  const acpInput = { write() { return true; }, end() { this.ended = true; } };
  const api = new Function('fixtureInput', 'acpInput', 'once', 'extensionMethods',
    `${inputSource([])}\nreturn forward();`)(fixtureInput, acpInput, async () => {}, []);
  await api;
  assert.equal(acpInput.ended, true);
});
