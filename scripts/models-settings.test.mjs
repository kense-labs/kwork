import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
const begin = source.indexOf('function modelNamespaceView('), end = source.indexOf('async function modelProviderOperation', begin);
const create = () => new Function(`let providerMutationTail = Promise.resolve();\n${source.slice(begin, end)}\nreturn modelSettingsRemote;`)();

test('Models returns only redacted namespace and credential view fields', async () => {
  const call = create();
  const descriptor = { ns: 'llm-pi-ai', schema: {}, value: {}, user: {}, revision: 0, applies: 'live',
    secrets: [{ path: ['secret'], set: true, literal: 'must-not-return' }], literal: 'must-not-return' };
  const host = { settings: { writable: true, describe(options) { assert.equal(options.redactSecrets, true); return [descriptor]; } },
    llm: { listConfigurableProviders: () => [] },
    credentials: { describe: async () => ({ configured: true, writable: false, masked: 'safe', literal: 'must-not-return' }) } };
  const configuration = await call(host, { method: 'settings/describe' });
  assert.equal(configuration.ok, true);
  assert.equal(JSON.stringify(configuration).includes('must-not-return'), false);
  const credential = await call(host, { method: 'credentials/describe', args: [['EXAMPLE_KEY']] });
  assert.deepEqual(credential.value, { EXAMPLE_KEY: { configured: true, writable: false, masked: 'safe' } });
  const invalid = await call(host, { method: 'credentials/describe', args: [Array(65).fill('EXAMPLE_KEY')] });
  assert.equal(invalid.error.code, 'gateway/bad-request');
});

test('Models serializes mutations and recovers its write queue after a refusal', async () => {
  const call = create();
  const observed = [];
  let release;
  const pending = new Promise(resolve => { release = resolve; });
  const host = { credentials: { async set(ref) { observed.push(ref); if (ref === 'FIRST') { await pending; throw Error('refused'); } } } };
  const first = call(host, { method: 'credentials/set', args: ['FIRST', 'one'] });
  const second = call(host, { method: 'credentials/set', args: ['SECOND', 'two'] });
  await Promise.resolve();
  assert.deepEqual(observed, ['FIRST']);
  release();
  assert.equal((await first).error.code, 'credential/rejected');
  assert.equal((await second).ok, true);
  assert.deepEqual(observed, ['FIRST', 'SECOND']);
});
