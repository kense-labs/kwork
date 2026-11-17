import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import test from 'node:test';

const source = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
const start = source.indexOf('async function settingsDocument(');
const end = source.indexOf('\nfunction runtimeSettings(', start);
assert(start >= 0 && end > start);
const document = new Function('isAbsolute', `${source.slice(start, end)}; return settingsDocument;`)(isAbsolute);
const ctx = settings => ({ get(name) { assert.equal(name, 'settings'); return settings; } });

test('document metadata checks the Host capability without preparing or opening it', async () => {
  assert.deepEqual(await document(ctx(undefined), { operation: 'describe' }), { available: false });
  let prepared = 0;
  const host = ctx({ prepareDocument() { prepared++; return '/owned/cordis.patch.yml'; } });
  assert.deepEqual(await document(host, { operation: 'describe' }), { available: true });
  assert.equal(prepared, 0);
  assert.deepEqual(await document(host, { operation: 'open', path: '/untrusted/other.yml' }), { path: '/owned/cordis.patch.yml' });
  assert.equal(prepared, 1);
});

test('invalid operations, missing providers and invalid Host paths fail closed', async () => {
  await assert.rejects(document(ctx(undefined), { operation: 'open' }), /unavailable/);
  await assert.rejects(document(ctx(undefined), { operation: 'edit' }), /Unknown/);
  for (const path of ['', 'relative.yml', null, 3]) {
    await assert.rejects(document(ctx({ prepareDocument: async () => path }), { operation: 'open' }), /Invalid/);
  }
  await assert.rejects(document(ctx({ prepareDocument: async () => { throw Error('prepare failed'); } }), { operation: 'open' }), /prepare failed/);
});
