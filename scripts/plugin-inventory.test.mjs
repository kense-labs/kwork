import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
const start = source.indexOf('async function pluginInventory(');
const end = source.indexOf('\nasync function pluginCenter(', start);
assert(start >= 0 && end > start);
const inventory = new Function(`${source.slice(start, end)}; return pluginInventory;`)();
const context = services => ({ baseUrl: 'file:///host/', get: name => services[name] });
const entry = (id, state) => ({ id, options: { name: `@fixture/${id}`, config: { apiKey: 'never-project-this' } },
  parent: { tree: { ctx: { baseUrl: 'file:///profile/' } } },
  fiber: state == null ? undefined : { state, await: async () => { throw Error('failed dependency'); } } });

test('inventory reads Loader without requiring plugin manager and excludes configuration secrets', async () => {
  const entries = Array.from({ length: 7 }, (_, state) => entry(String(state), state === 6 ? undefined : state));
  entries[1].disabled = true;
  const group = entry('group'); group.options.group = true;
  const host = context({ loader: { entries: () => [...entries, group] }, pluginPackages: {
    metaOf(name, base) { assert.equal(base, 'file:///profile/'); return { title: { zh: name, en: name } }; },
  } });
  const first = await inventory(host);
  assert.deepEqual(first.entries.map(row => row.fiberPhase), ['pending', 'loading', 'active', 'failed', null, 'unloading', null]);
  assert.equal(first.entries[1].enabled, false);
  assert.equal(first.entries[3].diagnostic, 'failed dependency');
  assert.equal(first.managementAvailable, undefined);
  assert.equal(first.agentPresets, undefined);
  assert.equal(JSON.stringify(first).includes('never-project-this'), false);
  entries[0].fiber.state = 2;
  assert.equal((await inventory(host)).entries[0].fiberPhase, 'active', 'each read observes the live Loader');
  await assert.rejects(inventory(context({})), /unavailable/);
});

test('optional preset composition preserves conditions and translates fiber phases', async () => {
  const preset = { id: 'standard', isDefault: true, rows: [
    { moduleName: '@fixture/conditional', entryId: null, enabled: 'conditional', condition: 'platform == windows' },
    { moduleName: '@fixture/live', entryId: 'live', enabled: true, fiberState: 2 },
  ] };
  const host = context({ loader: { entries: () => [] }, pluginManager: {},
    agentPresets: { compositionInventory: async () => [preset] },
    pluginPackages: { metaOf: name => ({ title: name }) },
  });
  const result = await inventory(host);
  assert.equal(result.managementAvailable, true);
  assert.equal(result.agentPresets[0].rows[0].condition, 'platform == windows');
  assert.equal(result.agentPresets[0].rows[0].enabled, 'conditional');
  assert.equal(result.agentPresets[0].rows[0].fiberPhase, null);
  assert.equal(result.agentPresets[0].rows[1].fiberPhase, 'active');
  assert.equal(result.agentPresets[0].rows[1].fiberState, undefined);
  assert.equal(preset.rows[1].fiberState, 2, 'projection leaves the Host roster unchanged');
});
