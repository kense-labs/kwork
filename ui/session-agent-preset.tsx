/** Official blank-session and header controls over KWork's ACP session binding. */
import { useEffect, useMemo, useSyncExternalStore } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { zh as common } from './vendor/harness-locale/zh';
import { zh } from './vendor/harness-agent-presets/locales';
import { AgentPresetSeat } from './vendor/harness-agent-presets/AgentPresetSeat';
import { AgentPresetLabel } from './vendor/harness-agent-presets/AgentPresetLabel';
import { AgentPresetSeatController, type AgentPresetStage } from './vendor/harness-agent-presets/seat-store';
import { AgentPresetSettingsController } from './vendor/harness-agent-presets/settings-store';
import { stageCanvasOverlay } from './document-html.js';

const dictionary = { ...common, ...zh };
const t = (key: keyof typeof dictionary, values: Record<string, string> = {}) =>
  dictionary[key].replace(/\{([^}]+)\}/g, (match, name) => values[name] ?? match);
const activeSelections = new Set<string>();
export function presetSelectionPending(session: string) { return activeSelections.has(session); }
const staged: AgentPresetStage = {id:undefined, introduce:false};
export function stagedAgentPreset() { return staged.id; }
export function stageCreatorAgentPreset() { staged.id = 'cordis'; staged.introduce = true; }

type Layout = {
  session: string | undefined; hero: boolean; developerTools: boolean; dark: boolean;
  bounds: [number, number, number, number];
};
type Owner = { holder: HTMLDivElement; root: Root; key: string; developerTools: boolean; seen: number };
let frame: Owner | undefined;
let generation = 0;

const invoke = (request: Record<string, unknown>) => (window as any).__TAURI__.core.invoke('agent_preset_settings', { request });

function SessionPreset({ session, hero, developerTools }: Omit<Layout, 'bounds'>) {
  const owner = useMemo(() => {
    const summary = { id: session, blank: false, projectionValues: { agentPreset: undefined as string | undefined } };
    let active = true;
    let revision = 0;
    const remote = { agentPresets: {
      list: async () => {
        try { return { ok: true, value: await invoke({operation: 'list'}) }; }
        catch (error) { return { ok: false, error: {message: String(error)} }; }
      },
      select: async (sessionId: string, id: string) => {
        if (!active || sessionId !== session) throw Error('会话已切换，请重新选择。');
        activeSelections.add(session);
        try {
          const result = await invoke({operation:'select', sessionId, id});
          summary.projectionValues.agentPreset = result.agentPreset;
          return {ok:true,value:result.agentPreset};
        } finally { activeSelections.delete(session); }
      },
    } };
    const ctx = {remote, locale:{bind:() => t}};
    const seat = new AgentPresetSeatController(ctx as any, () => session ? summary as any : undefined, staged);
    const label = new AgentPresetSettingsController(ctx as any);
    const load = async () => {
      const request = ++revision;
      if (!session) {
        if (hero && active) await seat.load();
        return;
      }
      try {
        const state = await invoke({operation:'session', sessionId:session});
        if (!active || request !== revision) return;
        summary.blank = state.canSelect === true;
        summary.projectionValues.agentPreset = state.agentPreset;
      } catch {
        if (!active || request !== revision) return;
        summary.blank = false;
      }
      if (!active || request !== revision) return;
      if (hero && summary.blank) {
        await seat.load();
        if (active && request === revision) await seat.apply();
      }
      else if (!hero && summary.projectionValues.agentPreset) await label.load();
    };
    return {
      mount: () => { active = true; void load(); },
      unmount: () => { active = false; revision++; },
      load, seat, label, summary,
      select: async (id: string) => {
        if (!active || (session && !summary.blank)) return '此会话已开始。';
        const outcome = await seat.select(id);
        return outcome.kind === 'refused' ? outcome.reason : undefined;
      },
    };
  }, [session, hero]);
  const seat = useSyncExternalStore(owner.seat.store.subscribe, owner.seat.store.getSnapshot);
  const label = useSyncExternalStore(owner.label.store.subscribe, owner.label.store.getSnapshot);
  useEffect(() => {
    owner.mount();
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void (window as any).__TAURI__.event.listen('acp-event', ({payload}: any) => {
      if (payload.kind === 'agent_presets_changed') void owner.load();
    }).then((cleanup: () => void) => { if (disposed) cleanup(); else unlisten = cleanup; });
    return () => { disposed = true; owner.unmount(); unlisten?.(); };
  }, [owner]);
  useEffect(() => {
    if (!developerTools && (staged.id === 'ptc' || staged.id === 'minimal')) {
      staged.id = undefined;
      staged.introduce = false;
      void owner.seat.apply();
    }
  }, [developerTools, owner]);
  return hero
    ? <AgentPresetSeat sessionId={session} t={t} load={owner.load} select={owner.select}
        introduced={() => owner.seat.introduced()} dismissRefusal={error => owner.seat.dismissRefusal(error)}
        useSessionRetainInfo={select => select({retainedBy:{mainView:1}})}
        useAgentPresetSeat={select => select(seat)} useDeveloperTools={select => select(developerTools)} />
    : session ? <AgentPresetLabel sessionId={session} t={t} load={owner.load}
        useSessions={select => select({byId:{[session]:owner.summary}})}
        useAgentPresets={select => select(label)} /> : null;
}

export function beginSessionPresetFrames() {
  const current = ++generation;
  queueMicrotask(() => {
    if (generation !== current || !frame || frame.seen === current) return;
    frame.root.unmount();
    frame.holder.remove();
    frame = undefined;
  });
}

export function stageSessionPreset(layout: Layout) {
  const {session, hero, developerTools, bounds, dark} = layout;
  document.body.toggleAttribute('data-ds-dark-theme', dark);
  const key = `${session}:${hero}`;
  if (!frame) {
    const holder = document.createElement('div');
    holder.dataset.sessionAgentPreset = '';
    holder.style.cssText = 'position:fixed;z-index:1;display:flex;align-items:center;pointer-events:none';
    document.body.append(holder);
    frame = {holder, root:createRoot(holder), key:'', developerTools, seen:generation};
    holder.addEventListener('pointerdown', event => {
      if (event.target instanceof Element && event.target.closest('button')) event.stopPropagation();
    });
  }
  frame.seen = generation;
  const [left, top, right, bottom] = bounds;
  Object.assign(frame.holder.style, {left:`${left}px`,top:`${top}px`,width:`${right-left}px`,height:`${bottom-top}px`});
  stageCanvasOverlay('session-agent-preset',frame.holder,bounds);
  if (frame.key !== key || frame.developerTools !== developerTools) {
    frame.key = key; frame.developerTools = developerTools;
    frame.root.render(<div style={{minWidth:0,maxWidth:'100%',pointerEvents:hero?'auto':'none'}}>
      <SessionPreset key={key} session={session} hero={hero} developerTools={developerTools} />
    </div>);
  }
}
