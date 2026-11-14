import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

// Exercise the real bridge functions without starting an ACP transport. This
// cuts immediately before `runProfile()` and appends no production exports.
async function harness(entries) {
  const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
  const body = source.slice(source.indexOf('const webFieldSections'), source.indexOf('async function checkpointTitle'));
  const factory = new Function(`class SessionOperationError extends Error {
    constructor(code, message = code) { super(message); this.code = code; }
  }\n${body}\nreturn { webSettings, webSettingsTail };`);
  const api = factory();
  const mutations = [];
  const credentials = [];
  const settings = {
    writable: true,
    describe() {
      return [...entries].map(([ns, entry]) => structuredClone({ ns, ...entry }));
    },
    async mutate(ns, edits, revision) {
      const entry = entries.get(ns);
      assert.equal(entry.revision, revision, 'mutation must use the latest validated revision');
      mutations.push({ ns, edits: structuredClone(edits), revision });
      for (const edit of edits) {
        if (edit.op === 'set') entry.value[edit.path[0]] = edit.value;
        else delete entry.value[edit.path[0]];
      }
      entry.revision++;
    },
  };
  const ctx = {
    get(name) {
      return {
        settings,
        web: {
          listProviders() {
            return Object.fromEntries(['search', 'fetch'].map(capability => [capability,
              [{ id: `deepseek-${capability}-api`, title: 'DeepSeek' }, { id: 'other', title: 'Other' }]]));
          },
        },
        credentials: {
          async describe(ref) { return { configured: false, writable: true, ref }; },
          async set(ref, key) { credentials.push({ ref, key }); },
        },
      }[name];
    },
  };
  const revisions = () => Object.fromEntries([...entries].map(([ns, entry]) => [ns, entry.revision]));
  return { ...api, ctx, entries, mutations, credentials, revisions };
}

function entry(value, revision = 1, base = value) {
  return { value: structuredClone(value), base: structuredClone(base), user: {}, revision };
}

function fixture() {
  return new Map([
    ['web', entry({ searchProvider: 'deepseek-search-api', fetchProvider: 'deepseek-fetch-api' })],
    ['web-deepseek-api', entry({ searchBilling: 'auto', fetchBilling: 'auto' })],
    ['web-search-deepseek', entry({ apiKeyEnv: 'DEEPSEEK_API_KEY' })],
  ]);
}

test('billing snapshot reports provider-specific advanced overrides', async () => {
  const h = await harness(fixture());
  const initial = await h.webSettings(h.ctx);
  assert.equal(initial.snapshot.searchBilling.value, 'auto');
  assert.deepEqual(initial.snapshot.searchBilling.advancedOverride, []);
  h.entries.get('web-deepseek-api').value.searchBaseURL = 'https://example.com';
  h.entries.get('web-deepseek-api').value.fetchApiKeyEnv = 'CUSTOM_KEY';
  const { snapshot } = await h.webSettings(h.ctx);
  assert.deepEqual(snapshot.searchBilling.advancedOverride, ['Base URL']);
  assert.deepEqual(snapshot.fetchBilling.advancedOverride, ['API Key 引用']);
});

test('billing saves once alongside provider change and returns fresh revisions', async () => {
  const h = await harness(fixture());
  const result = await h.webSettings(h.ctx, { change: {
    search: 'deepseek-search-api', searchBilling: 'account', revisions: h.revisions(),
  } });
  assert.equal(h.mutations.length, 2, 'provider and billing are each persisted exactly once');
  assert.deepEqual(h.mutations[0].edits, [{ op: 'set', path: ['searchBilling'], value: 'account' }]);
  assert.deepEqual(h.mutations[1].edits, [{ op: 'unset', path: ['searchProvider'] }]);
  assert.equal(result.failure, undefined);
  assert.equal(result.snapshot.searchBilling.value, 'account');
  assert.equal(result.snapshot.revisions.web, 2);
});

test('wrong providers, invalid modes and advanced overrides cannot write billing', async () => {
  for (const [change, expected] of [
    [{ search: 'other', searchBilling: 'account' }, /当前不可编辑/],
    [{ fetchBilling: 'invalid' }, /计费方式无效/],
    [{ fetch: 'other', fetchBilling: 'account' }, /当前不可编辑/],
  ]) {
    const h = await harness(fixture());
    await assert.rejects(h.webSettings(h.ctx, { change: { ...change, revisions: h.revisions() } }), expected);
    assert.equal(h.mutations.length, 0);
  }
  const h = await harness(fixture());
  h.entries.get('web-deepseek-api').value.searchBaseURL = 'https://example.com';
  await assert.rejects(h.webSettings(h.ctx, { change: {
    searchBilling: 'account', revisions: h.revisions(),
  } }), /当前不可编辑/);
  assert.equal(h.mutations.length, 0);
});

test('stale revisions reject before any mutation', async () => {
  const h = await harness(fixture());
  await assert.rejects(h.webSettings(h.ctx, { change: {
    searchBilling: 'account', revisions: { ...h.revisions(), 'web-deepseek-api': 0 },
  } }), /设置已更新/);
  assert.equal(h.mutations.length, 0);
  assert.equal(h.entries.get('web-deepseek-api').value.searchBilling, 'auto');
});

test('failed settings writes report a partial save and preserve the current snapshot', async () => {
  const h = await harness(fixture());
  const original = h.ctx.get('settings').mutate;
  h.ctx.get('settings').mutate = async (ns, edits, revision) => {
    if (ns === 'web') throw new Error('read-only deployment');
    await original(ns, edits, revision);
  };
  const result = await h.webSettings(h.ctx, { change: {
    search: 'deepseek-search-api', searchBilling: 'account', revisions: h.revisions(),
  } });
  assert.deepEqual(result.applied, ['searchBilling']);
  assert.match(result.failure, /未保存的内容已保留/);
  assert.equal(result.snapshot.search.value, 'deepseek-search-api');
  assert.equal(result.snapshot.searchBilling.value, 'account');
});
