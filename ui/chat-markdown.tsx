/** Layout/navigation adapter; Markdown parsing, DOM, and styling stay upstream. */
import { createRoot, type Root } from 'react-dom/client';
import { flushSync } from 'react-dom';
import { useMemo } from 'react';
import { MarkdownText, type MarkdownLabels } from './vendor/harness-primitives/markdown/MarkdownText';
import { MarkdownDelegateProvider } from './vendor/harness-primitives/markdown/MarkdownDelegate';
import { stageCanvasOverlay } from './document-html.js';
import { trajectoryFileDrag } from './trajectory-output';
import { markdownMenuBindings, prepareMarkdownMenu, retainMarkdownMenu } from './chat-context-menu';
import './chat-markdown.css';

type Rect = [number, number, number, number];
type Source = {
  session: string; workspace?: string; text: string; streaming: boolean; compact: boolean;
  mentions: { authored: string; resolved: string }[];
  images: { authored: string; path: string; url: string }[];
};
type Frame = {
  holder: HTMLDivElement; content: HTMLDivElement; root: Root; observer: ResizeObserver;
  source: string; session: string; height: number; seen: number; painted: number;
};
const frames = new Map<string, Frame>();
let generation = 0;
let sweep = 0;
let measureNotice = 0;
let notify: (action: string, value: string) => void;
const labels: MarkdownLabels = {
  code: { copyLabel: '复制', copiedLabel: '已复制', toolbarLabels: { codeLabel: '代码', wrapLabel: '自动换行', unwrapLabel: '取消换行' } },
  footnotes: '脚注',
};
function send(session: string, action: string, value: object = {}) {
  notify?.('chat-markdown', JSON.stringify({ session, action, ...value }));
}
export function listenChatMarkdown(callback: typeof notify) { notify = callback; }
export function beginChatMarkdownFrames() {
  generation++;
  // Hide old placements in the same task as the canvas repaint. Waiting for
  // the RAF sweep would leave the previous session's prose over the new shell
  // for a paint; prepaint reveals only rows belonging to this frame.
  for (const frame of frames.values()) frame.holder.style.visibility = 'hidden';
  if (sweep) return;
  sweep = requestAnimationFrame(() => {
    sweep = 0;
    for (const [key, frame] of frames) {
      if (frame.seen !== generation) {
        frame.observer.disconnect(); frame.root.unmount(); frame.holder.remove(); frames.delete(key);
      } else if (frame.painted !== generation) frame.holder.style.visibility = 'hidden';
    }
    retainMarkdownMenu(new Set([...frames.values()].filter(frame=>frame.painted===generation).map(frame=>frame.session)));
  });
}
function Body({ source }: { source: Source }) {
  const delegate = useMemo(() => {
    const getTriggerProps = markdownMenuBindings(source.session);
    const openFile = (path: string, options?: { line?: number }) => send(source.session, 'file', { path, ...options });
    const fileMentions = { resolve(token: string) {
      const value = token.trim();
      const matches = source.mentions.filter(file => file.authored === value || file.resolved === value
        || file.resolved.split(/[\\/]/).filter(Boolean).at(-1) === value);
      const path = matches[0]?.resolved;
      if (!path || matches.some(file => file.resolved !== path)) return;
      return { title: path, label: `打开 ${value}`, open: () => openFile(path), contextMenuProps:getTriggerProps({kind:'file',path}) };
    } };
    return { openFile, fileMentions, getTriggerProps,
      openExternalLink: (url: string) => send(source.session, 'url', { url }),
      pathImages: { resolve: (value: string) => source.images.find(image => image.authored === value)?.url },
      fileImages: { resolve: (path: string) => source.images.find(image => image.path === path)?.url,
        fileDrag: trajectoryFileDrag,
        labels: { dialog: '原图预览', close: '关闭原图预览', open: '查看原图', loading: '图片加载中…', failed: '图片加载失败' } },
    };
  }, [source.session, source.workspace, source.mentions, source.images]);
  return <MarkdownDelegateProvider {...delegate}>
    <MarkdownText text={source.text} streaming={source.streaming} labels={labels}
      fileMentions={delegate.fileMentions} pathImages={delegate.pathImages}
      variant={source.compact ? 'compact' : 'body'} textInteractionSource={!source.streaming} />
  </MarkdownDelegateProvider>;
}
function createFrame(key: string): Frame {
  const holder = document.createElement('div');
  holder.className = 'chat-markdown-frame'; holder.dataset.chatMarkdown = key;
  holder.style.visibility = 'hidden';
  const content = document.createElement('div'); content.className = 'chat-markdown-content';
  holder.append(content); document.body.append(holder);
  const frame: Frame = { holder, content, root: createRoot(content), source: '', session: '', height: 0,
    seen: generation, painted: -1, observer: new ResizeObserver(() => {
      const height = content.getBoundingClientRect().height;
      if (Math.abs(height - frame.height) < 0.01) return;
      frame.height = height;
      // Fonts, images, highlighting and wrap controls may resize after layout.
      // Never re-enter GPUI while React is rendering or a frame is measuring.
      if (!measureNotice) measureNotice = requestAnimationFrame(() => {
        measureNotice = 0; notify?.('chat-markdown-measure', '');
      });
    }),
  };
  frame.observer.observe(content);
  holder.addEventListener('wheel', event => {
    let node = event.target instanceof Element ? event.target : null;
    while (node && node !== holder) {
      if (node instanceof HTMLElement) {
        const style = getComputedStyle(node);
        const vertical = /auto|scroll/.test(style.overflowY);
        if (vertical && ['contain', 'none'].includes(style.overscrollBehaviorY)) return;
        if (vertical && node.scrollHeight > node.clientHeight && ((event.deltaY < 0 && node.scrollTop > 0)
          || (event.deltaY > 0 && node.scrollTop + node.clientHeight < node.scrollHeight - 1))) return;
        if (Math.abs(event.deltaX) > Math.abs(event.deltaY) && node.scrollWidth > node.clientWidth) return;
      }
      node = node.parentElement;
    }
    if (event.deltaY) {
      event.preventDefault(); send(frame.session, 'scroll', { delta: event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? innerHeight : 1) });
    }
  }, { passive: false });
  const modifiedLink = (event: MouseEvent) => {
    if (event.defaultPrevented || !(event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button === 1)) return;
    const anchor = event.target instanceof Element ? event.target.closest('a[href]') : null;
    if (anchor instanceof HTMLAnchorElement) { event.preventDefault(); send(frame.session, 'external', { url: anchor.href }); }
  };
  holder.addEventListener('click', modifiedLink); holder.addEventListener('auxclick', modifiedLink);
  holder.addEventListener('keydown', event => {
    if (event.defaultPrevented || event.isComposing || event.key !== 'Escape') return;
    event.preventDefault(); document.querySelector<HTMLElement>('#gpui-text-input')?.focus({ preventScroll: true });
    send(frame.session, 'focus');
  });
  return frame;
}
/** Measure at the final available width before GPUI places any following row. */
export function measureChatMarkdown(key: string, source: string, width: number, dark: boolean): number {
  let frame = frames.get(key);
  if (!frame) { frame = createFrame(key); frames.set(key, frame); }
  frame.seen = generation;
  document.body.toggleAttribute('data-ds-dark-theme', dark);
  frame.content.style.width = `${Math.max(1, width)}px`;
  if (source !== frame.source) {
    const data: Source = JSON.parse(source); frame.source = source; frame.session = data.session;
    prepareMarkdownMenu(data.session,data.workspace??'',send);
    flushSync(() => frame.root.render(<Body source={data} />));
  }
  frame.height = frame.content.getBoundingClientRect().height;
  return frame.height;
}
export function stageChatMarkdown(key: string, bounds: Rect, viewport: Rect, hidden: boolean) {
  const frame = frames.get(key);
  if (!frame) return;
  frame.seen = frame.painted = generation;
  const [x, y, right, bottom] = bounds;
  const left = Math.max(x, viewport[0]), top = Math.max(y, viewport[1]);
  const vr = Math.min(right, viewport[2]), vb = Math.min(bottom, viewport[3]);
  frame.holder.style.visibility = hidden || vr <= left || vb <= top ? 'hidden' : 'visible';
  Object.assign(frame.holder.style, { left: `${left}px`, top: `${top}px`, width: `${Math.max(0, vr - left)}px`, height: `${Math.max(0, vb - top)}px` });
  Object.assign(frame.content.style, { left: `${x - left}px`, top: `${y - top}px`, width: `${right - x}px` });
  stageCanvasOverlay(`chat-markdown-${key}`, frame.holder, [left, top, vr, vb]);
}
