import { test, expect } from 'bun:test';
import { trajectoryPreviewText } from '../ui/vendor/harness-trajectory/trajectory-preview.ts';

test('trajectory previews preserve code and JSON punctuation while projecting GFM', () => {
  expect(trajectoryPreviewText('{"command":"echo [ok] * x > y #1"}')).toBe('{"command":"echo [ok] * x > y #1"}');
  expect(trajectoryPreviewText('## 标题\n\n**重点** [链接](https://example.test)\n\n```sh\necho [ok] * x > y #1\n```')).toBe('标题 重点 链接 echo [ok] * x > y #1');
  expect(trajectoryPreviewText('![示意图](./fixture.png)\n\n<literal>')).toBe('示意图 <literal>');
});

test('trajectory preview ellipsis reflects truncation, including Markdown that projects to shorter text', () => {
  expect(trajectoryPreviewText('x'.repeat(512))).toBe('x'.repeat(512));
  expect(trajectoryPreviewText('x'.repeat(513))).toBe(`${'x'.repeat(512)}…`);
  expect(trajectoryPreviewText(' '.repeat(2048) + 'later')).toBe('…');
});
