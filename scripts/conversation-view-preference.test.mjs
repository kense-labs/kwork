import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const source = await readFile(new URL('../ui/src/bridge.rs', import.meta.url), 'utf8');
const start = source.indexOf('export function dshPreferredConversationView');
const end = source.indexOf('export function dshRandomId', start);
const create = new Function('localStorage', source.slice(start, end).replaceAll('export function ', 'function ')
  + '; return { read: dshPreferredConversationView, write: dshRememberConversationView };');
test('view persistence uses the official session key and preserves semantic drafts', () => {
  const data = new Map([['dsh.conversation.a', JSON.stringify({ draft: { text: 'keep @file', references: [{ id: 'attachment' }] }, view: 'chat' })]]);
  const store = { getItem: key => data.get(key) ?? null, setItem: (key, value) => data.set(key, value) };
  const { read, write } = create(store);
  write('a', 'trajectory'); assert.equal(read('a'), 'trajectory');
  assert.deepEqual(JSON.parse(data.get('dsh.conversation.a')).draft, { text: 'keep @file', references: [{ id: 'attachment' }] });
  assert.equal(read('b'), ''); write('b', 'chat'); assert.equal(read('b'), 'chat'); assert.equal(read('a'), 'trajectory');
});
test('unavailable or malformed browser storage does not break view switching', () => {
  const inaccessible = create({ getItem: () => { throw Error('denied'); }, setItem: () => { throw Error('denied'); } });
  assert.equal(inaccessible.read('a'), ''); assert.doesNotThrow(() => inaccessible.write('a', 'trajectory'));
  const malformed = create({ getItem: () => 'not json', setItem: () => { throw Error('must not clobber unreadable data'); } });
  assert.equal(malformed.read('a'), ''); assert.doesNotThrow(() => malformed.write('a', 'chat'));
});
