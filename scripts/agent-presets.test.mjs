import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { AgentPresetSectionController } from '../ui/vendor/harness-agent-presets/section-store.ts';

const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
const start = source.indexOf('async function agentPresetSettings(');
const end = source.indexOf('\nasync function pluginInventory(', start);
const gateway = new Function(`${source.slice(start, end)}; return agentPresetSettings;`)();
const deferred = () => Promise.withResolvers();

test('preset Host mutations require current revisions and valid, writable compositions', async () => {
  let revision = 1, selectedDefault = 'standard', writes = 0;
  const registry = {
    remoteExportList: async () => ({ presets: [{ id: selectedDefault, isDefault: true }] }),
    resolve: async id => ({ id, ...(id === 'broken' ? { broken: 'missing dependency' } : {}) }),
    readDocument: async id => ({ agentPreset: id, content: '- name: fixture' }),
  };
  const settings = { writable: true, describe: () => [{ ns: 'agent-preset-registry', revision, value: { secret: 'never-export' } }],
    async mutate(ns, operations, expected) {
      assert.equal(ns, 'agent-preset-registry'); assert.equal(expected, revision, 'revision conflict');
      assert.deepEqual(operations[0].path, ['selectedDefault']);
      selectedDefault = operations[0].value; revision++; writes++;
    } };
  const ctx = { get: name => ({ settings, agentPresets: registry })[name] };
  assert.equal(JSON.stringify(await gateway(ctx, { operation: 'list' })).includes('never-export'), false);
  assert.equal((await gateway(ctx, { operation: 'read', id: 'standard' })).content, '- name: fixture');
  await assert.rejects(gateway(ctx, { operation: 'default', id: 'broken', revision }), /missing dependency/);
  assert.equal(writes, 0);
  await gateway(ctx, { operation: 'default', id: 'minimal', revision });
  assert.equal(selectedDefault, 'minimal');
  await assert.rejects(gateway(ctx, { operation: 'default', id: 'standard', revision: 1 }), /revision conflict/);
  settings.writable = false;
  await assert.rejects(gateway(ctx, { operation: 'default', id: 'standard', revision }), /只读/);
  await assert.rejects(gateway(ctx, { operation: 'read', id: '' }), /标识/);
  await assert.rejects(gateway({ get: () => undefined }, { operation: 'list' }), /尚未就绪/);
});

test('preset selection targets the exact live Agent and preserves registry refusal', async () => {
  const agent = { session: {} }; let turns = 0;
  const agents = { get: id => id === 'live' ? agent : undefined,
    turnBoundaryOf: () => ({ openTurnStartSeq: null, lastTurn: turns }) };
  const registry = { presetForSession: () => 'standard',
    async select(target, id) { assert.equal(target, agent); if (turns) throw Error('locked'); return id; } };
  const ctx = { get: name => ({ agents, agentPresets: registry })[name] };
  assert.equal((await gateway(ctx, { operation: 'session', sessionId: 'live' })).canSelect, true);
  assert.equal((await gateway(ctx, { operation: 'select', sessionId: 'live', id: 'minimal' })).agentPreset, 'minimal');
  turns = 1;
  assert.equal((await gateway(ctx, { operation: 'session', sessionId: 'live' })).canSelect, false);
  await assert.rejects(gateway(ctx, { operation: 'select', sessionId: 'live', id: 'standard' }), /locked/);
  await assert.rejects(gateway(ctx, { operation: 'select', sessionId: 'closed', id: 'standard' }), /尚未就绪/);
});

test('official preset controller fences late viewers and single-flights accepted defaults', async () => {
  let rows = [{ id: 'standard', isDefault: true }, { id: 'minimal', isDefault: false }], writes = 0;
  const read = deferred(), write = deferred();
  const controller = new AgentPresetSectionController({ remote: {
    agentPresets: { list: async () => ({ ok: true, value: { presets: rows } }), read: () => read.promise },
    settings: { async update(_ns, changes) { writes++; await write.promise;
      rows = rows.map(row => ({ ...row, isDefault: row.id === changes.selectedDefault })); return { ok: true }; } },
  } });
  await controller.load();
  const view = controller.view('standard'); controller.closeView();
  read.resolve({ ok: true, value: { content: 'obsolete' } }); await view;
  assert.equal(controller.store.getSnapshot().view, null);
  const saving = controller.makeDefault('minimal'); await controller.makeDefault('standard');
  assert.equal(writes, 1); assert.equal(controller.store.getSnapshot().rows[0].isDefault, true);
  write.resolve(); await saving;
  assert.equal(controller.store.getSnapshot().rows[1].isDefault, true);
});

test('disabling coding tools resets only the hidden built-in default at the accepted revision', async () => {
  for (const named of [false, true]) {
    let rows = [{ id: 'standard', isDefault: false }, { id: 'minimal', ...(named ? { name: 'My custom preset' } : {}), isDefault: true }];
    let writes = 0;
    const controller = new AgentPresetSectionController({
      configForms: { get: () => ({ getSnapshot: () => ({ status: 'ready', writable: true, revision: 7 }) }) },
      remote: { agentPresets: { list: async () => ({ ok: true, value: { presets: rows } }) },
        settings: { async update(_ns, changes, revision) { writes++; assert.equal(revision, 7);
          assert.equal(changes.selectedDefault, 'standard'); rows = rows.map(row => ({ ...row, isDefault: row.id === 'standard' })); return { ok: true }; } } },
    });
    await controller.reconcileCodingTools(() => true);
    assert.equal(writes, named ? 0 : 1);
  }
});
