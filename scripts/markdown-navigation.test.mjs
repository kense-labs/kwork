import { test, expect } from 'bun:test';
import { readFileSync } from 'node:fs';
import { isAbsolute } from 'node:path';
import { markdownDestination } from '../ui/markdown-navigation.ts';

test('official file destinations retain decoded names and separate navigation lines', () => {
  for (const [value, path, line] of [
    ['src/file.ts:24:5', 'src/file.ts', 24],
    ['../My%20Notes.md#L24-L30', '../My Notes.md', 24],
    ['/tmp/中文.html', '/tmp/中文.html', undefined],
    ['docs/notes%3A24:30', 'docs/notes:24', 30],
    ['docs/file.ts:24#L3', 'docs/file.ts:24', 3],
    ['C:%5Cwork%5Cfile.ts:2', 'C:\\work\\file.ts', 2],
    ['file.ts#L0', 'file.ts', undefined],
    ['./file.ts:30-24', './file.ts', undefined],
    ['file%23name%3F.txt', 'file#name?.txt', undefined],
  ]) {
    expect(markdownDestination(value)).toEqual(line === undefined ? { path } : { path, line });
  }
});

test('ordinary URLs stay URLs while rejected destinations have no action', () => {
  expect(markdownDestination('https://example.com/a?q=1#L2')).toEqual({url:'https://example.com/a?q=1#L2'});
  expect(markdownDestination('mailto:person@example.com')).toEqual({url:'mailto:person@example.com'});
  for (const value of ['#L2', 'file.ts?raw=1', 'file%ZZ.ts', 'file%00.ts', 'javascript:alert', 'data:text/html,hi', 'blob:https://example.com/id', '%6Aavascript:123']) {
    expect(markdownDestination(value)).toBeUndefined();
  }
});

const bridge = readFileSync(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
const start = bridge.indexOf('function resolveSessionDocument(');
const end = bridge.indexOf('/** Resolve one live agent', start);
if (start < 0 || end < start) throw Error('missing document resolver');
const resolveDocument = new Function('isAbsolute', `${bridge.slice(start,end)};return resolveSessionDocument;`)(isAbsolute);

test('file clicks follow the live directory, preserve explicit absolute paths and reject invalid paths', () => {
  const agent = { session: { header: { cwd: '/workspace' } } };
  let current;
  const ctx = { get: () => ({ get: session => { expect(session).toBe(agent.session); return current; } }) };
  expect(resolveDocument(ctx,agent,'src/a.ts')).toEqual({path:'/workspace/src/a.ts',cwd:'/workspace'});
  current = '/workspace/moved';
  expect(resolveDocument(ctx,agent,'../a.ts')).toEqual({path:'/workspace/moved/../a.ts',cwd:'/workspace/moved'});
  expect(resolveDocument(ctx,agent,'/elsewhere/a.ts')).toEqual({path:'/elsewhere/a.ts',cwd:'/workspace/moved'});
  expect(() => resolveDocument(ctx,agent,'bad\0path')).toThrow();
});
