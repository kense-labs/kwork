import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { createUploadController } from '../ui/settings-upload-controller.ts';

const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
const start = source.indexOf('let sessionLogPreferenceTail =');
const end = source.indexOf('\nfunction runtimeSettings(', start);
assert(start >= 0 && end > start);
const gateway = new Function(`${source.slice(start, end)}; return sessionLogPreference;`)();
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };

test('Host serializes preference writes, fences stale revisions and projects no unrelated fields', async () => {
  let value = false, revision = 1;
  const gate = deferred(), entered = deferred();
  let calls = 0;
  const settings = { writable: true, describe: () => [{ ns: 'session-log-deepseek', revision,
    value: { enabled: value, token: 'never-return' } }],
    async mutate(ns, ops, expected) {
      calls++; if (calls === 1) { entered.resolve(); await gate.promise; }
      assert.equal(ns, 'session-log-deepseek');
      if (expected !== revision) throw Error('SETTINGS_CONFLICT');
      assert.deepEqual(ops[0].path, ['enabled']); value = ops[0].value; revision++;
    } };
  const ctx = { get: () => settings };
  const first = gateway(ctx, { operation: 'set', enabled: true, revision: 1 });
  await entered.promise;
  const stale = gateway(ctx, { operation: 'set', enabled: false, revision: 1 });
  assert.equal(calls, 1);
  gate.resolve(); assert.equal((await first).value.enabled, true);
  await assert.rejects(stale, /CONFLICT/);
  const read = await gateway(ctx, { operation: 'read' });
  assert.equal(read.revision, 2); assert.equal(JSON.stringify(read).includes('never-return'), false);
  settings.writable = false;
  await assert.rejects(gateway(ctx, { operation: 'set', enabled: false, revision: 2 }), /read-only/);
  await assert.rejects(gateway(ctx, { operation: 'set', enabled: 'true', revision: 2 }), /Invalid/);
  assert.deepEqual(await gateway({ get: () => undefined }, { operation: 'read' }), { available: false });
});

test('upload keeps the accepted switch during save, rejects duplicates, and reports failure after refresh', async () => {
  let saved = false, fail = false, writes = 0;
  const gate = deferred();
  const controller = createUploadController(async request => {
    if (request.operation === 'set') {
      writes++; await gate.promise;
      if (fail) throw Error('conflict'); saved = request.enabled;
    }
    return { available: true, status: 'ready', writable: true, revision: 3, value: { enabled: saved } };
  });
  await controller.load();
  const pending = controller.preference.setEnabled(true);
  await controller.preference.setEnabled(false);
  assert.equal(writes, 1); assert.equal(controller.form.getSnapshot().value.enabled, false);
  gate.resolve(); await pending;
  assert.equal(controller.form.getSnapshot().value.enabled, true);
  assert.equal(controller.preference.state.getSnapshot().notice, 'saved');
  fail = true; await controller.preference.setEnabled(false);
  assert.equal(controller.form.getSnapshot().value.enabled, true);
  assert.equal(controller.preference.state.getSnapshot().notice, 'failed');
});

test('old connection replies cannot publish or enable writes on the replacement', async () => {
  const pending = deferred(); let calls = 0;
  const controller = createUploadController(() => { calls++; return pending.promise; });
  const read = controller.load(); controller.dispose();
  pending.resolve({ available: true, status: 'ready', writable: true, revision: 7, value: { enabled: true } });
  await read; assert.equal(controller.form.getSnapshot().available, false);
  await controller.preference.setEnabled(true); assert.equal(calls, 1);
});
