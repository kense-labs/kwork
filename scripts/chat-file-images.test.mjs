import { test, expect } from 'bun:test';
import { markdownImagePaths } from '../ui/markdown-navigation.ts';

test('file previews use the official grammar for local links, pictures and references', () => {
  const source = '[图片](../images/a%20b.png) ![内联](./nested/image.webp)\n\n[引用][picture]\n\n[picture]: /tmp/中文.svg\n\n```md\n[代码](ignored.png)\n```\n\n`[行内](ignored2.png)` [文档](readme.md) ![远程](https://example.com/x.png)';
  expect(markdownImagePaths(source)).toEqual(['../images/a b.png','./nested/image.webp','/tmp/中文.svg']);
  expect(markdownImagePaths('[同一张](./a.png) ![同一张](./a.png) [危险](javascript:foo.png)')).toEqual(['./a.png']);
});
