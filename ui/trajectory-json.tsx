export { readTrajectoryImageDrag } from './trajectory-output';
export { officialModelsEnabled, updateSettingsModels, placeSettingsModels } from './settings-models';
export { listenClientPreferences, updateClientPreferencesConnection, saveClientTheme, refreshClientPreferences } from './client-preferences';
export { updateSettingsPlugins, placeSettingsPlugins } from './settings-plugins';
export { listenSettingsAgentPresets, updateSettingsAgentPresets, placeSettingsAgentPresets } from './settings-agent-presets';
export { listenSettingsUpload, updateSettingsUpload, placeSettingsUpload, settingsUploadHeight } from './settings-upload';
export { focusWithoutRing } from './vendor/harness-primitives/focus';
export { listenSettingsTheme, updateSettingsTheme, placeSettingsTheme, placeSettingsThemeViewport, settingsThemeHeight } from './settings-theme';
/** GPUI layout adapter for the unmodified upstream JSON inspector. */
import { ToolCatalog, SystemPromptDiff } from './trajectory-prompt';
import { stageCanvasOverlay } from './document-html.js';
import { beginChatMarkdownFrames, listenChatMarkdown } from './chat-markdown';
export { measureChatMarkdown, stageChatMarkdown } from './chat-markdown';
import { beginTrajectoryTimelineFrames, listenTrajectoryTimeline } from './trajectory-timeline';
export { stageTrajectoryTimeline, trajectoryTimelineHeight, trajectoryTimelineFocus, trajectoryTimelineHistoryStarted, focusTrajectorySearch } from './trajectory-timeline';
import { createRoot, type Root } from 'react-dom/client';
import { JsonTree, type JsonTreeLabels } from './vendor/harness-primitives/JsonTree';
import { ICON_SYMBOLS } from './vendor/harness-primitives/icons/artwork';
import './vendor/harness-primitives/base.css';
import './vendor/harness-primitives/design-platform.css';
import './vendor/harness-primitives/gradient-shadow-text.css';
import './trajectory-json.css';
import './vendor/harness-primitives/shiki.css';
import { ProgramInput, ProgramOutput, programTranslate } from './trajectory-program';
import { ToolOutput } from './trajectory-output';
import { codeProgram } from './vendor/harness-trajectory/code-program';
import { trajectoryPreviewText } from './vendor/harness-trajectory/trajectory-preview';
import { beginTrajectoryAccessibilityFrames, listenTrajectoryAccessibility } from './trajectory-accessibility';
export { stageTrajectoryAccessibility } from './trajectory-accessibility';

// Native rows and the upstream inspector share the same bounded GFM projection.
// Cache only the bounded source so a retained transcript is not duplicated here.
const previewCache = new Map<string, string>();
export function trajectoryRecordPreview(text: string) {
  const key = `${text.length > 2048 ? 'long:' : 'short:'}${text.slice(0, 2048)}`;
  const cached = previewCache.get(key);
  if (cached !== undefined) return cached;
  const result = trajectoryPreviewText(text);
  if (previewCache.size >= 512) previewCache.delete(previewCache.keys().next().value!);
  previewCache.set(key, result);
  return result;
}

type Rect = [number, number, number, number];
type Layout = { bounds: Rect; viewport: Rect; dark: boolean; preview: boolean; hidden: boolean; kind: string };
type Frame = { holder: HTMLDivElement; content: HTMLDivElement; root: Root; observer: ResizeObserver;
  source: string; kind: string; preview: boolean; height: number; seen: number; painted: number };
const frames = new Map<string, Frame>();
let generation = 0;
let sweep = 0;
let notify: (action: string, value: string) => void;
const wrapPreference = 'dsh.trajectory.jsonStringWrapping';
let wrap = false;
try { wrap = localStorage.getItem(wrapPreference) === 'true'; } catch { /* private storage may be unavailable */ }
const labels: JsonTreeLabels = {
  copyValue: '复制值', copyJson: '复制 JSON', copyPath: '复制属性路径',
  copyPrettyJson: '复制格式化 JSON', copyCompactJson: '复制紧凑 JSON',
  copied: '已复制', copyFailed: '复制失败', collapseNode: '收起', expandNode: '展开',
  copyButtonTitle: action => `${action}；右键点击可选择复制方式`,
};
const stringWrapping = { getDefault: () => wrap, setDefault: (value: boolean) => {
  wrap = value;
  try { localStorage.setItem(wrapPreference, JSON.stringify(value)); } catch { /* keep the current session preference */ }
},
  label: '自动换行' };
function send(action: string, value = '') { notify?.(action, value); }
function restoreShell() { document.querySelector<HTMLElement>('#gpui-text-input')?.focus({ preventScroll: true }); }
export function listenTrajectoryJson(callback: typeof notify) {
  notify = callback; installIcons(); listenTrajectoryTimeline(callback); listenTrajectoryAccessibility(callback); listenChatMarkdown(callback);
}
export function beginTrajectoryJsonFrames() {
  beginChatMarkdownFrames();
  beginTrajectoryTimelineFrames();
  beginTrajectoryAccessibilityFrames();
  generation++;
  if (sweep) return;
  sweep = requestAnimationFrame(() => {
    sweep = 0;
    for (const [key, frame] of frames) {
      if (frame.seen !== generation) {
        frame.observer.disconnect(); frame.root.unmount(); frame.holder.remove(); frames.delete(key);
      } else if (frame.painted !== generation) frame.holder.hidden = true;
    }
    if (separator && separatorSeen !== generation) { separator.remove(); separator = undefined; drag = undefined; }
  });
}
export function trajectoryJsonHeight(key: string) {
  const frame = frames.get(key);
  if (frame) frame.seen = generation;
  return frame?.height ?? 62;
}
function controls(container: HTMLElement) {
  return [...container.querySelectorAll<HTMLElement>('button:not(:disabled), summary, [tabindex="0"]')]
    .filter(node => node.getClientRects().length && !node.closest('[hidden], [inert]'));
}
export function focusTrajectoryJson(backward: boolean) {
  const nodes = [...frames.values()].flatMap(frame => controls(frame.holder));
  const target = backward ? nodes.at(-1) : nodes[0];
  target?.focus({ preventScroll: true });
  return !!target;
}
function installIcons() {
  if (document.getElementById('harness-json-icons')) return;
  const namespace = 'http://www.w3.org/2000/svg';
  const sprite = document.createElementNS(namespace, 'svg');
  sprite.id = 'harness-json-icons'; sprite.setAttribute('aria-hidden', 'true');
  sprite.style.cssText = 'position:absolute;width:0;height:0;overflow:hidden';
  for (const [id, markup] of Object.entries(ICON_SYMBOLS)) {
    const source = new DOMParser().parseFromString(markup, 'image/svg+xml').documentElement;
    const symbol = document.createElementNS(namespace, 'symbol');
    for (const { name, value } of source.attributes) {
      if (!['width', 'height', 'xmlns', 'class', 'aria-hidden'].includes(name)) symbol.setAttribute(name, value);
    }
    symbol.id = id; symbol.append(...[...source.childNodes].map(node => document.importNode(node, true))); sprite.append(symbol);
  }
  document.body.append(sprite);
}
function createFrame(key: string) {
  installIcons();
  const holder = document.createElement('div'); holder.className = 'trajectory-json-frame';
  holder.dataset.trajectoryControl = '';
  const content = document.createElement('div'); holder.append(content); document.body.append(holder);
  const root = createRoot(content);
  const frame: Frame = { holder, content, root, height: 62, source: '', kind: '', preview: false, seen: generation, painted: generation,
    observer: new ResizeObserver(() => {
      const height = Math.ceil(content.getBoundingClientRect().height);
      if (height > 0 && height !== frame.height) {
        frame.height = height;
        requestAnimationFrame(() => { if (frames.get(key) === frame) send('measure'); });
      }
    }),
  };
  frame.observer.observe(content);
  // React's handlers run first. Only unconsumed boundary keys return to GPUI.
  content.addEventListener('keydown', event => {
    if (event.defaultPrevented || event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.key === 'Escape') { event.preventDefault(); restoreShell(); send('close'); }
    if (event.key === 'Tab') {
      const nodes = [...frames.values()].flatMap(item => controls(item.holder));
      const edge = event.shiftKey ? nodes[0] : nodes.at(-1);
      if (document.activeElement === edge) {
        event.preventDefault(); restoreShell(); send(event.shiftKey ? 'tab-before' : 'tab-after');
      }
    }
  });
  holder.addEventListener('wheel', event => {
    // Let a raw string or horizontal tree scrollbar consume its own gesture.
    let node = event.target instanceof Element ? event.target : null;
    while (node && node !== holder) {
      if (node instanceof HTMLElement) {
        const style = getComputedStyle(node);
        const scrollContainer = /auto|scroll/.test(style.overflowY);
        // Respect the upstream inspector's scroll containment even at the first
        // or last line. Forwarding that boundary gesture to GPUI would scroll
        // the entire source panel, including its fixed toolbar.
        if (scrollContainer && (style.overscrollBehaviorY === 'contain' || style.overscrollBehaviorY === 'none')) return;
        const vertical = scrollContainer && node.scrollHeight > node.clientHeight;
        if (vertical && ((event.deltaY < 0 && node.scrollTop > 0)
          || (event.deltaY > 0 && node.scrollTop + node.clientHeight < node.scrollHeight - 1))) return;
        if (Math.abs(event.deltaX) > Math.abs(event.deltaY) && node.scrollWidth > node.clientWidth) return;
      }
      node = node.parentElement;
    }
    if (event.deltaY) {
      event.preventDefault(); send('scroll', String(event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? innerHeight : 1)));
    }
  }, { passive: false });
  return frame;
}
export function stageTrajectoryJson(key: string, source: string, layout: Layout) {
  let frame = frames.get(key);
  if (layout.hidden) {
    if (frame) { frame.seen = frame.painted = generation; frame.holder.hidden = true; }
    return;
  }
  if (!frame) { frame = createFrame(key); frames.set(key, frame); }
  frame.seen = frame.painted = generation; frame.holder.hidden = false;
  frame.holder.dataset.trajectoryKind = layout.kind;
  document.body.toggleAttribute('data-ds-dark-theme', layout.dark);
  const [x, y, right] = layout.bounds;
  const [vx, vy, vr, vb] = layout.viewport;
  const left = Math.max(x, vx), top = Math.max(y, vy);
  Object.assign(frame.holder.style, { left: `${left}px`, top: `${top}px`,
    width: `${Math.max(0, Math.min(right, vr) - left)}px`, height: `${Math.max(0, vb - top)}px` });
  Object.assign(frame.content.style, { position: 'relative', left: `${x - left}px`, top: `${y - top}px`, width: `${right - x}px` });
  stageCanvasOverlay(`trajectory-json-${key}`, frame.holder, [left, top, Math.min(right, vr), vb]);
  const fullProgram = layout.kind === 'program-input' && !layout.preview;
  frame.content.classList.toggle('trajectory-program-full', fullProgram);
  frame.content.style.height = fullProgram ? `${Math.max(24, vb - y)}px` : '';
  if (frame.source !== source || frame.preview !== layout.preview || frame.kind !== layout.kind) {
    frame.source = source; frame.preview = layout.preview; frame.kind = layout.kind;
    const data = JSON.parse(source);
    if (layout.kind === 'program-input' && data.schemaDetail === null) delete data.schemaDetail;
    if (layout.kind === 'program-output' && data.cell.outputDetail === null) delete data.cell.outputDetail;
    const program = layout.kind === 'program-input' ? codeProgram(data) : undefined;
    const openProgram = (tab: string) => { restoreShell(); send('program-open', tab); };
    frame.root.render(program !== undefined
      ? <ProgramInput key={key} program={program} initialWrapped={wrap} stringWrapping={stringWrapping}
          onOpen={layout.preview ? () => openProgram('Input') : undefined} t={programTranslate} />
      : layout.kind === 'program-output'
        ? <ProgramOutput key={key} record={data} stringWrapping={stringWrapping}
            onOpen={layout.preview ? () => openProgram('Output') : undefined} t={programTranslate} />
      : layout.kind === 'tool-output'
        ? <ToolOutput value={data.value} blocks={data.blocks} error={data.error} preview={layout.preview} labels={labels} stringWrapping={stringWrapping} />
      : layout.kind.startsWith('literal-')
        ? <pre className={`trajectory-literal-payload${layout.preview ? ' trajectory-literal-preview' : ''}`}>{data.value}</pre>
      : layout.kind === 'prompt-tools'
      ? <ToolCatalog key={key} tools={data} stringWrapping={stringWrapping} labels={labels} />
      : layout.kind === 'prompt-diff'
        ? <SystemPromptDiff key={key} before={data.before} after={data.after} />
        : layout.kind === 'raw-result-scalar'
          ? <pre className="trajectory-result-scalar" aria-label="原始结果">{JSON.stringify(data)}</pre>
        : <JsonTree key={key} data={data} label="JSON 内容" labels={labels}
          collapsedStringLines={layout.preview ? 3 : 12} stringWrapping={stringWrapping} />);
  }
}

let separator: HTMLDivElement | undefined;
export function focusTrajectorySeparator() {
  if (!separator?.isConnected || separator.closest('[inert]')) return false;
  separator.focus({ preventScroll: true }); return true;
}
let separatorSeen = 0;
let splitWidth = 0;
let panelWidth = 0;
let drag: { pointerId: number; x: number; width: number; split: number } | undefined;
function clampWidth(width: number, split: number) { return Math.round(Math.min(Math.max(width, 320), Math.max(320, Math.min(720, split - 280)))); }
export function stageTrajectorySeparator(rect: Rect, split: number, width: number, hidden: boolean) {
  if (hidden) return;
  if (!separator) {
    separator = document.createElement('div');
    separator.className = 'trajectory-detail-resize'; separator.dataset.trajectoryControl = '';
    separator.setAttribute('role', 'separator'); separator.setAttribute('aria-orientation', 'vertical');
    separator.setAttribute('aria-label', '调整事件详情宽度');
    separator.title = '拖动调整大小；双击恢复默认值。'; separator.tabIndex = 0;
    separator.addEventListener('dblclick', () => send('resize', 'reset'));
    separator.addEventListener('focus', () => { separator!.setAttribute('aria-valuenow', String(Math.round(panelWidth))); });
    separator.addEventListener('pointerdown', event => {
      if (event.button !== 0) return;
      drag = { pointerId: event.pointerId, x: event.clientX, width: panelWidth, split: splitWidth };
      separator!.focus({ preventScroll: true }); separator!.setPointerCapture(event.pointerId); event.preventDefault();
    });
    separator.addEventListener('pointermove', event => {
      if (drag?.pointerId !== event.pointerId) return;
      send('resize', String(clampWidth(drag.width + drag.x - event.clientX, drag.split)));
    });
    separator.addEventListener('pointerup', event => {
      if (drag?.pointerId !== event.pointerId) return;
      drag = undefined; separator!.releasePointerCapture(event.pointerId);
    });
    separator.addEventListener('pointercancel', () => { drag = undefined; });
    separator.addEventListener('lostpointercapture', () => { drag = undefined; });
    separator.addEventListener('keydown', event => {
      if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
        event.preventDefault(); send('resize', String(clampWidth(panelWidth + (event.key === 'ArrowLeft' ? 16 : -16), splitWidth)));
      } else if (event.key === 'Escape') { event.preventDefault(); restoreShell(); send('close'); }
      else if (event.key === 'Tab') { event.preventDefault(); restoreShell(); send(event.shiftKey ? 'tab-record' : 'tab-header'); }
    });
    document.body.append(separator);
  }
  separatorSeen = generation; splitWidth = split; panelWidth = width;
  separator.setAttribute('aria-valuenow', String(Math.round(panelWidth)));
  Object.assign(separator.style, { left: `${rect[0] - 4}px`, top: `${rect[1]}px`, height: `${rect[3] - rect[1]}px` });
}

export { listenSettingsPermission, updateSettingsPermission, placeSettingsPermission, settingsPermissionHeight } from "./settings-permission";

export { listenSettingsDeveloper, updateSettingsDeveloper, placeSettingsDeveloper, settingsDeveloperHeight } from "./settings-developer";
