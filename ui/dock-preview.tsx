/** Mount official read-only content in the measured GPUI dock body. */
import { useEffect, useMemo, useRef, type ReactNode } from 'react';
import { flushSync } from 'react-dom';
import { createRoot, type Root } from 'react-dom/client';
import { stageCanvasOverlay } from './document-html.js';
import { openExternalFileLink } from './native-file-actions';
import { MarkdownDelegateProvider } from './vendor/harness-primitives/markdown/MarkdownDelegate';
import { zh as common } from './vendor/harness-locale/zh';
import { useLocalMarkdownImages } from './chat-file-images';
import { markdownImageDestinations } from './markdown-navigation';
import { MarkdownBody } from './vendor/harness-document-markdown/MarkdownBody';
import { zh as markdown } from './vendor/harness-document-markdown/locales';
import css from './dock-preview.module.css';
import { PlanPreview } from './vendor/harness-plan/PlanPreview';
import { zh } from './vendor/harness-plan/locales';
import { reviewPreviewAddress } from './vendor/harness-plan/review-preview';
import type { PlanDocument, PlanResource, PropsLocale } from './vendor/harness-plan/runtime-types';

type Rect = [number, number, number, number];
type Cover = { bounds: Rect; radius: number; shadow: boolean; visual_only: boolean };
type Layout = { bounds: Rect; viewport: Rect; covers: Cover[]; radius: number; dark: boolean; hidden: boolean; inert: boolean };
type PlanSource = {
  session: string;
  key: string;
  callId?: string;
  document?: PlanDocument;
  status: PlanResource['status'];
  failure?: PlanResource['failure'];
};
type DocumentSource = {
  session: string; key: string; path: string; text: string; eof: boolean;
  pending: boolean; failure?: string; canLoad: boolean;
};
type Frame = { holder: HTMLDivElement; content: HTMLDivElement; root: Root; source?: string; seen: number };

const dictionary = { ...common, ...zh };
const translate: PropsLocale<'plan'>['t'] = key => dictionary[key];
const frames = new Map<string, Frame>();
let generation = 0;
let notify: ((action: string, value: string) => void) | undefined;
const markdownDictionary = { ...common, ...markdown };
const documentText = (key: keyof typeof markdownDictionary) => markdownDictionary[key];
const scrollOffsets = new Map<string, number>();
const openExternal = (url: string) => { void openExternalFileLink(url).catch(() => {}); };

export function listenDockPreview(callback: NonNullable<typeof notify>) { notify = callback; }

export function beginDockPreviewFrames() {
  const current = ++generation;
  queueMicrotask(() => {
    if (generation !== current) return;
    for (const [key, frame] of frames) {
      if (frame.seen === current) continue;
      frame.root.unmount();
      frame.holder.remove();
      frames.delete(key);
    }
  });
}

function Plan({ source }: { source: PlanSource }) {
  const { session, key, callId, document, status, failure } = source;
  const address = callId
    ? `dsh-resource://plan/${encodeURIComponent(session)}/${encodeURIComponent(callId)}`
    : reviewPreviewAddress(session, key);
  const resource: PlanResource = { status, failure: failure ?? undefined,
    value: callId && document ? { ...document, callId } : undefined };
  const params = !callId && document ? { planReview: document } : undefined;
  return <MarkdownDelegateProvider openExternalLink={openExternal}>
    <PlanPreview useTabInfo={() => ({ tab: { title: document?.title ?? zh['preview.title'], navigation: { address, params } } })}
      useResource={() => resource} t={translate} />
  </MarkdownDelegateProvider>;
}

/** Official Markdown previews resolve images beside the document, not the session cwd. */
function imagePath(base: string, destination: string) {
  const suffix = destination.search(/[?#]/u);
  let path: string;
  try { path = decodeURIComponent(suffix < 0 ? destination : destination.slice(0, suffix)); }
  catch { return undefined; }
  if (!path || /[\u0000-\u001f\u007f]/u.test(path)) return undefined;
  if (/^[a-z]:[/\\]/iu.test(path) || /^[/\\]/u.test(path)) return path;
  if (/^[a-z][a-z\d+.-]*:/iu.test(path)) return undefined;
  return base.slice(0, Math.max(base.lastIndexOf('/'), base.lastIndexOf('\\')) + 1) + path;
}

function Document({ source, frameKey }: { source: DocumentSource; frameKey: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const resolve = useMemo(() => (path: string) => path, [source.path]);
  const paths = useMemo(() => source.eof ? markdownImageDestinations(source.text)
    .flatMap(destination => imagePath(source.path, destination) ?? []) : [], [source.path, source.text, source.eof]);
  const images = useLocalMarkdownImages(paths, resolve);
  const pathImages = useMemo(() => ({
    resolve: (destination: string) => images.get(imagePath(source.path, destination) ?? ''),
  }), [images, source.path]);
  const more = () => notify?.('dock-preview-more', JSON.stringify({session: source.session, key: source.key}));
  useEffect(() => {
    if (ref.current) ref.current.scrollTop = Math.max(ref.current.scrollTop, scrollOffsets.get(frameKey) ?? 0);
  }, [frameKey, source.text]);
  return <div ref={ref} className={css.scrollport} tabIndex={0} aria-label="Markdown"
    onScroll={event => {
      if (event.target !== event.currentTarget) return;
      const body = event.currentTarget;
      const end = body.scrollTop + body.clientHeight >= body.scrollHeight - 1;
      const previous = scrollOffsets.get(frameKey) ?? 0;
      if (!end || body.scrollTop >= previous) {
        if (!scrollOffsets.has(frameKey) && scrollOffsets.size >= 128)
          scrollOffsets.delete(scrollOffsets.keys().next().value!);
        scrollOffsets.set(frameKey, body.scrollTop);
      }
      if (end && source.canLoad && !source.failure && !source.pending) more();
    }}>
    <MarkdownDelegateProvider openExternalLink={openExternal}>
      <MarkdownBody content={{kind: 'text', text: source.text, eof: source.eof}} pathImages={pathImages} t={documentText} />
    </MarkdownDelegateProvider>
    {source.failure ? <p className={css.statusLine} role="status">
      <span>{source.failure}</span>
      <button className={css.action} type="button" disabled={!source.canLoad || source.pending} onClick={more}>重试</button>
    </p> : !source.eof ? <button className={css.more} disabled={source.pending || !source.canLoad}
      type="button" onClick={more}>{source.pending ? '加载中…' : '加载更多'}</button> : null}
  </div>;
}

export function stagePlanPreview(key: string, source: string, layout: Layout) {
  stagePreview('plan', key, source, layout, () => <Plan source={JSON.parse(source)} />);
}

export function stageDocumentMarkdown(key: string, source: string, layout: Layout) {
  const documentKey = key.slice(key.lastIndexOf(':') + 1);
  stagePreview('document', key, source, layout, () => <Document source={JSON.parse(source)} frameKey={key} />, documentKey);
}

function stagePreview(kind: 'plan' | 'document', key: string, serialized: string, layout: Layout,
  render: () => ReactNode, documentKey?: string) {
  key = kind + ':' + key;
  const [x, y, right, bottom] = layout.bounds;
  const left = Math.max(x, layout.viewport[0]);
  const top = Math.max(y, layout.viewport[1]);
  const vr = Math.min(right, layout.viewport[2]);
  const vb = Math.min(bottom, layout.viewport[3]);
  let frame = frames.get(key);
  if (!frame) {
    if (layout.hidden || vr <= left || vb <= top) return;
    const holder = document.createElement('div');
    holder.dataset.dockPreview = kind;
    holder.style.cssText = 'position:fixed;overflow:hidden;z-index:1;pointer-events:auto';
    const content = document.createElement('div');
    content.style.position = 'relative';
    holder.append(content);
    document.body.append(holder);
    frame = { holder, content, root: createRoot(content), seen: generation };
    frames.set(key, frame);
  }
  frame.seen = generation;
  frame.holder.hidden = layout.hidden || vr <= left || vb <= top;
  frame.holder.inert = frame.holder.hidden || layout.inert;
  if (frame.holder.hidden) return;
  document.body.toggleAttribute('data-ds-dark-theme', layout.dark);
  Object.assign(frame.holder.style, { left: `${left}px`, top: `${top}px`, width: `${vr - left}px`,
    height: `${vb - top}px`, borderRadius: layout.radius ? `0 0 ${layout.radius}px ${layout.radius}px` : '' });
  Object.assign(frame.content.style, { left: `${x - left}px`, top: `${y - top}px`,
    width: `${right - x}px`, height: `${bottom - y}px` });
  stageCanvasOverlay(`dock-preview-${key}`, frame.holder, [left, top, vr, vb], layout.covers, documentKey);
  if (frame.source !== serialized) {
    frame.source = serialized;
    const { root } = frame;
    flushSync(() => root.render(render()));
  }
}
