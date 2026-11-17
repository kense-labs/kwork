import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
const begin = source.indexOf('async function readModelCatalog');
const end = source.indexOf('function observeModelProviders', begin);
const readModelCatalog = new Function(`${source.slice(begin, end)}\nreturn readModelCatalog;`)();
const draftStart = source.indexOf('function configuredModels');
const draftEnd = source.indexOf('\nfunction installedProvider(', draftStart);
const configuredModels = new Function('providerError', `${source.slice(draftStart, draftEnd)}\nreturn configuredModels;`)(message => new Error(message));

test('retry editor reads the installed defaults and supports older schemas', () => {
  const start = source.indexOf('function providerRetrySpec');
  const end = source.indexOf('\nasync function publicProviders', start);
  const project = new Function(`${source.slice(start, end)}\nreturn providerRetrySpec;`)();
  const number = (min, max, value) => ({ meta: { min, max, default: value } });
  const delay = { maxDelayMs: number(undefined, 1000, 800), jitterRatio: number(0, 1, 0.2) };
  const normal = { dict: { mode: { value: 'normal' }, maxRetries: number(0, 500, 3),
    retryableCodes: { meta: { default: ['SERVER'] }, inner: { type: 'string' } },
    backoff: { dict: { ...delay, initialDelayMs: number(undefined, 1000, 200) } },
  } };
  const always = { dict: { mode: { value: 'always' },
    backoff: { dict: { ...delay, initialDelayMs: number(0.1, 1000, 10) } },
  } };
  const older = project({ list: [normal, always] });
  assert.equal(older.normalBackoff.initialDelayMs.min, Number.MIN_VALUE);
  assert.equal(older.alwaysBackoff.initialDelayMs.min, 0.1);
  assert.equal(older.maxRetries.value, 3);
  assert.deepEqual(older.defaultCodes, ['SERVER']);
  assert.ok(older.choices.includes('QUOTA'));
  normal.dict.retryableCodes.inner = { list: [{ type: 'const', value: 'ADAPTER_BUSY' }] };
  assert.deepEqual(project({ list: [normal, always] }).choices, ['ADAPTER_BUSY']);
  assert.equal(project({ list: [normal] }), null);
});

test('shared provider editing follows the Host directory and credential ownership', async () => {
  const start = source.indexOf('function isPiAiProvider');
  const end = source.indexOf('// A configured compatible route', start);
  const { installedProviders, customProviderIds, userOwnsProvider, providerCanEdit, canClearProviderKey, removeUnusedProviderKey } =
    new Function('providerSettings', 'managedProvider', 'editableProtocols', 'managedCredential',
      `${source.slice(start, end)}
       return { installedProviders, customProviderIds, userOwnsProvider, providerCanEdit, canClearProviderKey, removeUnusedProviderKey };`)(
      'llm-pi-ai', /^kwork-[a-f0-9-]{36}$/, new Set(['openai-completions']), /^KWORK_[A-F0-9]{32}_API_KEY$/);
  const profile = { api: 'openai-completions', baseURL: 'https://fixture.invalid/v1', apiKeyEnv: 'OFFICIAL_GATEWAY_API_KEY' };
  const legacyId = 'kwork-fdfe2386-8ffc-4c1a-b09d-1ceddd16c9b9';
  const profiles = { 'official-gateway': profile, 'launch-gateway': profile, openai: profile, [legacyId]: profile };
  const entry = (provider, declared, settingsNs = 'llm-pi-ai') => ({
    provider, declared, settingsNs, settingsPath: ['providers', provider],
  });
  const llm = { listConfigurableProviders: () => [
    entry('official-gateway', true), entry('openai', false), entry('launch-gateway', true, 'different-llm'),
  ] };
  assert.deepEqual(customProviderIds({ llm }, profiles), ['official-gateway']);
  assert.deepEqual(installedProviders({ llm }).map(row => row.provider), ['openai']);
  assert.deepEqual(customProviderIds({ llm: {} }, profiles), [legacyId]);
  const config = { user: { providers: { 'official-gateway': profile } }, base: { providers: { 'base-gateway': profile } } };
  assert.equal(userOwnsProvider(config, 'official-gateway'), true);
  assert.equal(userOwnsProvider(config, 'base-gateway'), false);
  assert.equal(providerCanEdit(config, 'base-gateway'), true);
  assert.equal(providerCanEdit(config, 'launch-gateway'), false);
  assert.equal(canClearProviderKey(config, 'official-gateway', profile), true);
  assert.equal(canClearProviderKey(config, 'base-gateway', profile), false);
  assert.equal(canClearProviderKey(config, 'launch-gateway', profile), false);

  let settings = [{ value: { providers: { paired: profile } } }];
  const removed = [];
  const ctx = { settings: { describe: () => settings }, credentials: {
    describe: async () => ({ writable: true }), unset: async ref => removed.push(ref),
  } };
  await removeUnusedProviderKey(ctx, 'official-gateway', profile.apiKeyEnv);
  assert.deepEqual(removed, []);
  settings = [];
  await removeUnusedProviderKey(ctx, 'other-gateway', profile.apiKeyEnv);
  assert.deepEqual(removed, []);
  await removeUnusedProviderKey(ctx, 'official-gateway', profile.apiKeyEnv);
  assert.deepEqual(removed, [profile.apiKeyEnv]);
});

test('provider authorization keeps the device link and withdraws a losing prompt', async () => {
  const start = source.indexOf('function providerLoginUrl');
  const end = source.indexOf('function startProviderLogin', start);
  const { providerLoginUrl, providerLoginPrompt, providerLoginNotice } = new Function('randomUUID',
    `${source.slice(start, end)}\nreturn { providerLoginUrl, providerLoginPrompt, providerLoginNotice };`)(() => 'prompt-fixture');
  assert.equal(providerLoginUrl('http://localhost:8080/callback'), 'http://localhost:8080/callback');
  assert.equal(providerLoginUrl('http://remote.fixture/'), undefined);
  assert.equal(providerLoginUrl('https://secret@fixture.invalid/'), undefined);
  const attempt = { controller: new AbortController(), view: { revision: 0, status: 'running' } };
  providerLoginNotice(attempt, { message: 'Enter the code', url: 'https://fixture.invalid/verify', code: 'ABCD' });
  providerLoginNotice(attempt, { message: 'Still waiting', url: 'javascript:alert(1)' });
  assert.equal(attempt.view.url, 'https://fixture.invalid/verify');
  assert.equal(attempt.view.code, 'ABCD');
  assert.equal(attempt.view.message, 'Still waiting');

  const callback = new AbortController();
  const answer = providerLoginPrompt(attempt, { kind: 'secret', message: 'Token', signal: callback.signal });
  assert.equal(attempt.view.prompt.id, 'prompt-fixture');
  assert.equal(Object.hasOwn(attempt.view.prompt, 'signal'), false);
  const rejected = assert.rejects(answer, /won the race/);
  callback.abort(new Error('Browser callback won the race'));
  await rejected;
  assert.equal(attempt.pending, undefined);
  assert.equal(attempt.view.prompt, undefined);
  assert.equal(attempt.controller.signal.aborted, false);
  assert.equal(attempt.view.revision, 4);
});

test('compatible model edits clear overrides and keep hidden fields when the ID changes', () => {
  const previous = [{
    id: 'vision', name: 'Vision', contextWindow: 2300000, maxTokens: 32000, input: ['text'],
    compat: { supportsStore: false }, reasoningEfforts: { off: null, high: 'high' },
  }, { id: 'other', name: 'Old model' }];
  const edited = [
    { id: 'new-vision', sourceId: 'vision', name: null, contextWindow: null, maxTokens: null, inputModalities: ['image'] },
    { id: 'other', sourceId: null, name: null, contextWindow: null, maxTokens: null },
  ];
  assert.deepEqual(configuredModels(edited, previous), [
    { id: 'new-vision', input: ['image'], compat: previous[0].compat, reasoningEfforts: previous[0].reasoningEfforts },
    { id: 'other' },
  ]);
  assert.deepEqual(configuredModels([{ id: 'vision', name: null, maxTokens: null }], previous), previous.slice(0, 1));
  assert.throws(() => configuredModels([{ ...edited[0], contextWindow: 2.5 }], previous), /模型令牌数/);
  assert.throws(() => configuredModels([edited[0], { id: ' new-vision ' }], previous), /模型 ID 必须唯一/);
  assert.deepEqual(configuredModels([], previous, undefined, true), []);
  assert.throws(() => configuredModels([], previous), /请添加 1 到 512/);
});

test('model reasoning distinguishes inheritance, explicit disable and ordered provider values', () => {
  const model = { id: 'reasoned' };
  const previous = [{ ...model, reasoningEfforts: false }];
  const write = reasoningEfforts => configuredModels([{ ...model, reasoningEfforts }], previous)[0];
  assert.deepEqual(configuredModels([model], previous), previous);
  assert.deepEqual(write(null), model);
  assert.equal(write(false).reasoningEfforts, false);
  const entries = [['medium', 'vendor-medium'], ['off', null], ['low', 'vendor-low']];
  assert.deepEqual(Object.entries(write(entries).reasoningEfforts), entries);
  assert.throws(() => write([['off', 'vendor-off']]), /Off 以外/);
  assert.throws(() => write([['high', null]]), /参数值不能为空/);
  assert.throws(() => write([['high', '']]), /参数值不能为空/);
  assert.throws(() => write([['high', 'high'], ['high', 'other']]), /有效且唯一/);
  assert.throws(() => write([['custom', 'high']]), /有效且唯一/);
  assert.throws(() => write(true), /配置无效/);

  const google = 'google-generative-ai';
  assert.deepEqual(configuredModels([{ ...model, reasoningEfforts: [['high', 'high']] }], previous, google), [
    { ...model, reasoningEfforts: { high: 'high' } },
  ]);
  assert.deepEqual(configuredModels([model], previous, google), previous);
  assert.throws(() => configuredModels([{ ...model, reasoningEfforts: [['high', 'custom']] }], previous, google), /Gemini/);
  assert.throws(() => configuredModels([model], [{ ...model, reasoningEfforts: { off: null, high: 'high' } }], google), /Gemini/);
});

test('catalog isolates listing and exact-route failures and can recover on retry', async () => {
  let recovered = false;
  const reads = [];
  const llm = {
    listProviders: () => ['deepseek-account', 'healthy', 'offline', 'broken-model', 'empty']
      .map(id => ({ id, name: id })),
    async listModels(provider) {
      reads.push(provider);
      if (provider === 'offline' && !recovered) throw new Error('listing timeout');
      if (provider === 'empty') return [];
      return [{ id: 'same-id', name: `${provider} model`, description: 'Public note' }];
    },
    async resolveModelInfo(provider, id) {
      assert.equal(id, 'same-id');
      if (provider === 'broken-model') throw new Error('invalid model');
    },
  };
  const ctx = { get: () => llm };
  const first = await readModelCatalog(ctx);
  assert.deepEqual(reads, ['healthy', 'offline', 'broken-model', 'empty']);
  assert.deepEqual(first, {
    available: true,
    groups: [{
      group: 'healthy',
      name: 'healthy',
      options: [{ value: '["healthy","same-id"]', name: 'healthy model', description: 'Public note' }],
    }],
    failures: [
      { id: 'offline', name: 'offline', message: 'listing timeout' },
      { id: 'broken-model', name: 'broken-model', message: 'invalid model' },
    ],
  });
  recovered = true;
  const retried = await readModelCatalog(ctx);
  assert.deepEqual(retried.groups.map(group => group.group), ['healthy', 'offline']);
  assert.equal(retried.groups[1].options[0].value, '["offline","same-id"]');
  assert.deepEqual(retried.failures.map(failure => failure.id), ['broken-model']);
});

test('account models are discovered only while the Host reports a stored account', async () => {
  let status = 'signed-out';
  const reads = [];
  const llm = {
    listProviders: () => [{ id: 'deepseek-account', name: 'DeepSeek Account' }, { id: 'deepseek-official', name: 'DeepSeek' }],
    async listModels(provider) {
      reads.push(provider);
      return [{ id: 'deepseek-v4-pro', name: 'DeepSeek V4 Pro' }];
    },
    async resolveModelInfo() {},
  };
  const ctx = { get: service => service === 'llm' ? llm : { getState: async () => ({ status }) } };
  assert.deepEqual((await readModelCatalog(ctx)).groups.map(group => group.group), ['deepseek-official']);
  assert.deepEqual(reads, ['deepseek-official']);
  status = 'credential-stored';
  const signedIn = await readModelCatalog(ctx);
  assert.deepEqual(signedIn.groups.map(group => group.group), ['deepseek-account', 'deepseek-official']);
  assert.equal(signedIn.groups[0].options[0].value, '["deepseek-account","deepseek-v4-pro"]');
  status = 'signed-out';
  assert.deepEqual((await readModelCatalog(ctx)).groups.map(group => group.group), ['deepseek-official']);
});
