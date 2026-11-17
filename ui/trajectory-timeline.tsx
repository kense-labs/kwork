/** Measured GPUI host for the upstream timeline and its projection. */
import { createRoot, type Root } from 'react-dom/client';
import { useEffect, useState } from 'react';
import { stageCanvasOverlay } from './document-html.js';
import { TrajectoryTimeline } from './vendor/harness-trajectory/TrajectoryTimeline';
import { TrajectoryToolbar } from './vendor/harness-trajectory/TrajectoryToolbar';
import { trajectoryTimelineFocusIndexes, type TrajectoryTimelineMode, type TrajectoryTimeRange } from './vendor/harness-trajectory/timeline';
import { zh, type TrajectoryTranslate } from './vendor/harness-trajectory/locales';
import type { TrajectoryTurnModel } from './vendor/harness-trajectory/layout';

type Rect = [number, number, number, number];
type Layout = { bounds: Rect; viewport: Rect; dark: boolean; hidden: boolean; focused: boolean };
type Data = {
  session: string; turns: TrajectoryTurnModel[]; mode: TrajectoryTimelineMode;
  query: string; folds: [boolean, boolean, boolean];
  range: [number, number] | null; selectedIndex: number | null; matches: number[] | null;
  hasEarlier: boolean; historyLoading: boolean; historyRequest: number; historyError: boolean;
};
type Frame = { holder: HTMLDivElement; content: HTMLDivElement; root: Root; observer: ResizeObserver;
  session: string; seen: number; source: string; focused: boolean };
let frame: Frame | undefined;
let generation = 0;
let sweep = 0;
let measuredHeight = 84;
let searchFocusRequested = false;
let notify: (action: string, value: string) => void;
let cachedSource = '';
let cachedData: Data;
let turnsSource = '';
let stableTurns: TrajectoryTurnModel[];
let serial = 0;
const pending = new Map<string, { session: string; request?: number; resolve: (ok: boolean) => void }>();
const t: TrajectoryTranslate = (key, values) => zh[key].replace(/\{(\w+)\}/g, (match, name) => String(values?.[name] ?? match));
function dataFor(source: string) {
  if (source !== cachedSource) {
    const data: Data = JSON.parse(source);
    const nextTurns = JSON.stringify(data.turns);
    if (turnsSource !== nextTurns) { turnsSource = nextTurns; stableTurns = data.turns; }
    data.turns = stableTurns;
    cachedSource = source; cachedData = data;
  }
  return cachedData;
}
function send(session: string, action: string, value: unknown = null) {
  notify?.('timeline', JSON.stringify({ session, action, value }));
}
function restoreShell() { document.querySelector<HTMLElement>('#gpui-text-input')?.focus({ preventScroll: true }); }
function settle(id: string, ok: boolean) { const item = pending.get(id); pending.delete(id); item?.resolve(ok); }
function removeFrame() {
  if (!frame) return;
  for (const [id, item] of pending) if (item.session === frame.session) settle(id, false);
  frame.observer.disconnect(); frame.root.unmount(); frame.holder.remove(); frame = undefined;
}
export function listenTrajectoryTimeline(callback: typeof notify) { notify = callback; }
export function beginTrajectoryTimelineFrames() {
  generation++;
  if (sweep) return;
  sweep = requestAnimationFrame(() => {
    sweep = 0;
    if (frame && frame.seen !== generation) removeFrame();
  });
}
export function trajectoryTimelineHeight() { return measuredHeight; }
export function focusTrajectorySearch() { searchFocusRequested = true; }
export function trajectoryTimelineFocus(source: string) {
  const data = dataFor(source);
  return JSON.stringify(data.range === null ? null : [...trajectoryTimelineFocusIndexes(data.turns,
    { start: data.range[0], end: data.range[1] }, data.mode)]);
}
/** An acknowledgement identifies the actual single-flight host request. */
export function trajectoryTimelineHistoryStarted(id: string, request: number, loading: boolean) {
  const item = pending.get(id);
  if (!item) return;
  if (!loading) settle(id, false);
  else item.request = request;
}
function loadEarlier(session: string) {
  return new Promise<boolean>(resolve => {
    const id = String(++serial);
    pending.set(id, { session, resolve });
    send(session, 'load', id);
  });
}

function TrajectoryOverview({ data }: { data: Data }) {
  const [query, setQuery] = useState(data.query);
  useEffect(() => { setQuery(data.query); }, [data.query]);
  const onRangeChange = (range: TrajectoryTimeRange | null) => send(data.session, 'range', range);
  return <>
    <TrajectoryToolbar t={t} actualDuration={data.mode === 'duration' || data.mode === 'actual'}
      onActualDurationChange={value => send(data.session, 'duration', value)}
      actualTime={data.mode === 'time' || data.mode === 'actual'} onActualTimeChange={value => send(data.session, 'time', value)}
      allTurnsCollapsed={data.folds[0]} allAssistantsCollapsed={data.folds[1]} allSubCallsCollapsed={data.folds[2]}
      onToggleAllTurns={() => send(data.session, 'fold', '$turn')}
      onToggleAllAssistants={() => send(data.session, 'fold', '$call')}
      onToggleAllSubCalls={() => send(data.session, 'fold', '$subcall')}
      searchQuery={query} onSearchQueryChange={value => { setQuery(value); send(data.session, 'query', value); }} />
    <TrajectoryTimeline t={t} turns={data.turns} mode={data.mode}
      range={data.range === null ? null : { start: data.range[0], end: data.range[1] }}
      selectedIndex={data.selectedIndex} searchMatchIndexes={data.matches === null ? null : new Set(data.matches)}
      hasEarlierRecords={data.hasEarlier} onLoadEarlier={() => loadEarlier(data.session)}
      onRangeChange={onRangeChange} onRecordSelect={index => { restoreShell(); send(data.session, 'select', index); }}
      onRecordFocus={index => send(data.session, 'focus', index)} />
  </>;
}
export function stageTrajectoryTimeline(source: string, layout: Layout) {
  const data = dataFor(source);
  if (frame && frame.session !== data.session) removeFrame();
  for (const [id, item] of pending) {
    if (item.session === data.session && item.request !== undefined
      && data.historyRequest >= item.request && !data.historyLoading) settle(id, !data.historyError);
  }
  if (!frame) {
    const holder = document.createElement('div');
    holder.className = 'trajectory-timeline-frame'; holder.dataset.trajectoryControl = '';
    const content = document.createElement('div'); holder.append(content); document.body.append(holder);
    const current: Frame = { holder, content, root: createRoot(content), session: data.session,
      seen: generation, source: '', focused: false, observer: new ResizeObserver(() => {
        const height = content.getBoundingClientRect().height;
        if (height > 0 && Math.abs(height - measuredHeight) > 0.1) {
          measuredHeight = height;
          requestAnimationFrame(() => { if (frame === current) send(current.session, 'measure'); });
        }
      }) };
    frame = current; current.observer.observe(content);
    // Let React consume Escape and internal Tab stops before returning to GPUI.
    holder.addEventListener('keydown', event => {
      if (event.defaultPrevented || event.isComposing || event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key === 'Tab') {
        const nodes = [...content.querySelectorAll<HTMLElement>('button, input, [tabindex="0"]')]
          .filter(node => node.getClientRects().length && !node.closest('[hidden], [inert]'));
        if (document.activeElement === (event.shiftKey ? nodes[0] : nodes.at(-1))) {
          event.preventDefault(); restoreShell(); send(current.session, 'tab', event.shiftKey);
        }
      }
    });
  }
  frame.seen = generation;
  frame.holder.hidden = layout.hidden;
  document.body.toggleAttribute('data-ds-dark-theme', layout.dark);
  const [x, y, right, bottom] = layout.bounds;
  const [vx, vy, vr, vb] = layout.viewport;
  const left = Math.max(x, vx), top = Math.max(y, vy);
  Object.assign(frame.holder.style, { left: `${left}px`, top: `${top}px`,
    width: `${Math.max(0, Math.min(right, vr) - left)}px`, height: `${Math.max(0, Math.min(bottom, vb) - top)}px` });
  Object.assign(frame.content.style, { position: 'relative', left: `${x - left}px`, top: `${y - top}px`, width: `${right - x}px` });
  stageCanvasOverlay('trajectory-timeline', frame.holder, [left, top, Math.min(right, vr), Math.min(bottom, vb)]);
  if (frame.source !== source) {
    frame.source = source;
    frame.root.render(<TrajectoryOverview key={data.session} data={data} />);
  }
  if ((searchFocusRequested || layout.focused && !frame.focused) && !layout.hidden) {
    const current = frame;
    const search = searchFocusRequested;
    searchFocusRequested = false;
    requestAnimationFrame(() => {
      if (frame === current && current.focused && !current.holder.hidden) {
        current.content.querySelector<HTMLElement>(search ? 'input[type="search"]' : 'button')?.focus({ preventScroll: true });
      }
    });
  }
  frame.focused = layout.focused;
}
