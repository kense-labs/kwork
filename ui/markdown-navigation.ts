/** Markdown destinations use the same parser and URL policy as Harness 39163dd. */
import { normalizeUri } from 'micromark-util-sanitize-uri';
import { isExplicitFilePath, parseFileLink } from './vendor/harness-primitives/markdown/file-link';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server.browser';
import { classifyLinkPath, LinkIconMedium } from './vendor/harness-primitives/LinkIcon';

const icons = new Map<string, string>();
/** Use upstream artwork for canvas links too, including the known-site marks. */
export function markdownLinkIcon(value: string, file: boolean): string {
  const kind = file ? classifyLinkPath(value) : 'url';
  const host = file ? '' : (() => { try { return new URL(value).hostname; } catch { return ''; } })();
  const key = `${kind}:${host}`;
  let svg = icons.get(key);
  if (svg === undefined) {
    svg = renderToStaticMarkup(createElement(LinkIconMedium, { kind, href: file ? undefined : value, size: 16 }));
    if (icons.size >= 256) icons.delete(icons.keys().next().value!);
    icons.set(key, svg);
  }
  return svg;
}

export function markdownDestination(value: string): { path: string; line?: number } | { url: string } | undefined {
  const file = parseFileLink(value);
  if (file !== undefined) return file;
  // Policy from ui-primitives/markdown/render.tsx::sanitizeUrl. Do not turn
  // rejected local destinations or embedded-document protocols into OS opens.
  const url = normalizeUri(value);
  if (isExplicitFilePath(url)) return undefined;
  try {
    if (['javascript:', 'vbscript:', 'data:', 'blob:'].includes(new URL(url).protocol)) return undefined;
    return { url };
  } catch { return undefined; }
}
