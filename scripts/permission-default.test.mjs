import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { createPermissionController } from '../ui/settings-permission-controller.ts';

const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
const start = source.indexOf('let permissionDefaultTail =');
const end = source.indexOf('\nlet sessionLogPreferenceTail =', start);
const gateway = new Function(`${source.slice(start, end)}; return permissionDefault;`)();
const options = ['read-only', 'workspace-write', 'danger-full-access'].map(value => ({ value, name: value }));
const snapshot = (preset = 'workspace-write', revision = 1) => ({ view: { writable: true,
  namespaces: [{ ns: 'permission', revision, value: { defaultPreset: preset } }] },
  catalog: { defaultPreset: preset, defaultOptions: options } });
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };

test('Host defaults use advertised new-session choices and revision/acknowledgement fences', async () => {
  let preset = 'workspace-write', revision = 1, writes = 0;
  const settings = { writable: true, describe: () => [{ ns: 'permission', revision, value: { defaultPreset: preset, secret: 'not-projected' } }],
    async mutate(ns, ops, expected) {
      assert.equal(ns, 'permission'); assert.deepEqual(ops[0].path, ['defaultPreset']);
      if (expected !== revision) throw Error('CONFLICT');
      writes++; preset = ops[0].value; revision++;
    } };
  const permission = { catalog: () => ({ defaultPreset: preset, defaultOptions: options, options: [...options, { value: 'auto', name: 'auto' }] }) };
  const ctx = { get: name => ({ settings, permissionPresets: permission })[name] };
  assert.equal(JSON.stringify(await gateway(ctx, { operation: 'read' })).includes('not-projected'), false);
  await assert.rejects(gateway(ctx, { operation: 'set', preset: 'auto', revision }), /advertised/);
  await assert.rejects(gateway(ctx, { operation: 'set', preset: 'danger-full-access', revision }), /confirmation/);
  assert.equal(writes, 0);
  const saved = await gateway(ctx, { operation: 'set', preset: 'read-only', revision });
  assert.equal(saved.view.namespaces[0].value.defaultPreset, 'read-only');
  await assert.rejects(gateway(ctx, { operation: 'set', preset: 'workspace-write', revision: 1 }), /CONFLICT/);
  settings.writable = false;
  await assert.rejects(gateway(ctx, { operation: 'set', preset: 'workspace-write', revision }), /read-only/);
  assert.deepEqual((await gateway({ get: () => undefined }, { operation: 'read' })).view.namespaces, []);
});

test('official controller retains accepted defaults, single-flights saves, and recovers failures', async () => {
  let accepted = snapshot(), calls = 0, gate = deferred();
  const owner = createPermissionController(async request => {
    if (request.operation === 'read') return accepted;
    calls++; await gate.promise;
    accepted = snapshot(request.preset, accepted.view.namespaces[0].revision + 1); return accepted;
  });
  await owner.load();
  const save = owner.select('read-only');
  await owner.select('danger-full-access');
  assert.equal(calls, 1); assert.equal(owner.controller.store.getSnapshot().currentValue, 'workspace-write');
  gate.resolve(); await save;
  assert.equal(owner.controller.store.getSnapshot().currentValue, 'read-only');
  gate = deferred(); const failure = owner.select('workspace-write'); gate.reject(Error('fixture conflict')); await failure;
  assert.equal(owner.controller.store.getSnapshot().currentValue, 'read-only');
  assert.equal(owner.controller.store.getSnapshot().status, 'error');
  await owner.load(); assert.equal(owner.controller.store.getSnapshot().status, 'ready');
  owner.dispose(); await owner.select('workspace-write'); assert.equal(calls, 2);
});

test('disposed connection ignores late loads', async () => {
  const gate = deferred(); const owner = createPermissionController(() => gate.promise);
  const load = owner.load(); owner.dispose(); gate.resolve(snapshot('danger-full-access')); await load;
  assert.equal(owner.controller.store.getSnapshot().currentValue, '');
});
