/** Official preset settings over the current Host connection. */
import { useSyncExternalStore } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { AgentPresetSection } from './vendor/harness-agent-presets/AgentPresetSection';
import { AgentPresetSectionController } from './vendor/harness-agent-presets/section-store';
import { zh } from './vendor/harness-agent-presets/locales';
import { zh as common } from './vendor/harness-locale/zh';

type View = { visible: boolean; connected: boolean; generation: number; locked: boolean; developerTools: boolean;
  developerLoaded: boolean; blankSession?: string };
let view: View;
let holder: HTMLDivElement;
let root: Root;
let bounds: number[];
let dispatch: (action: string, value: string) => void;
let owner: ReturnType<typeof createOwner> | undefined;
let instance = 0;
let listening = false;
const dictionary = { ...common, ...zh };
const t = (key: keyof typeof dictionary, values: Record<string, string> = {}) =>
  dictionary[key].replace(/\{([^}]+)\}/g, (match, name) => values[name] ?? match);

function createOwner() {
  const lifetime = { active: true };
  let document: { status: string; revision?: number; writable: boolean } = { status: 'idle', writable: false };
  const invoke = async (request: Record<string, unknown>) => {
    if (!lifetime.active) throw new Error('连接已更换，请重试。');
    const result = await (window as any).__TAURI__.core.invoke('agent_preset_settings', { request });
    if (!lifetime.active) throw new Error('连接已更换，请重试。');
    if (request.operation === 'list' || request.operation === 'default') {
      document = { status: 'ready', writable: result.writable, revision: result.revision };
    }
    return { ok: true, value: result };
  };
  const remote = {
    agentPresets: { list: () => invoke({ operation: 'list' }), read: (id: string) => invoke({ operation: 'read', id }) },
    settings: { update: (_namespace: string, changes: { selectedDefault: string }, revision?: number) =>
      invoke({ operation: 'default', id: changes.selectedDefault, revision: revision ?? document.revision }) },
  };
  const controller = new AgentPresetSectionController({ remote,
    configForms: { get: () => ({ getSnapshot: () => document }) }, locale: { bind: () => t },
  } as any);
  const shouldReset = () => lifetime.active && view?.developerLoaded === true && !view.developerTools;
  let revision = 0;
  let reconciling: Promise<void> | undefined;
  const reconcile = () => {
    revision++;
    if (!shouldReset()) return;
    reconciling ??= Promise.resolve().then(async () => {
      try {
        while (shouldReset()) {
          const requested = revision;
          await controller.load();
          await controller.reconcileCodingTools(shouldReset);
          if (requested === revision) return;
        }
      } finally { reconciling = undefined; }
    });
  };
  return { lifetime, controller, load: () => controller.load(), reconcile, id: ++instance,
    useSnapshot: (select: (value: any) => any) => select(useSyncExternalStore(controller.store.subscribe, controller.store.getSnapshot)),
    read: (id: string) => controller.view(id), close: () => controller.closeView(),
    save: (id: string) => {
      const session = view?.blankSession;
      return controller.makeDefault(id, async preset => {
        if (!session || !lifetime.active || view?.blankSession !== session) return;
        const state = (await invoke({operation:'session', sessionId:session})).value;
        if (state.canSelect && state.agentPreset !== preset && view?.blankSession === session)
          await invoke({operation:'select', sessionId:session, id:preset});
      });
    },
  };
}

export function listenSettingsAgentPresets(callback: typeof dispatch) { dispatch = callback; }
export function officialAgentPresetsEnabled() {
  const platform = document.documentElement.dataset.platform;
  return platform === 'darwin' || platform === 'win32';
}
export function placeSettingsAgentPresets(left: number, top: number, width: number, height: number) {
  bounds = [left, top, width, height];
  if (holder) for (const [index, key] of ['left', 'top', 'width', 'height'].entries()) holder.style[key as any] = `${bounds[index]}px`;
}
export function updateSettingsAgentPresets(next: View) {
  if (!officialAgentPresetsEnabled()) return;
  const previous = view; view = next;
  if (!next.connected || previous?.generation !== next.generation) {
    if (owner) owner.lifetime.active = false;
    owner = undefined;
  }
  if (next.connected) {
    owner ??= createOwner();
    if (!previous?.connected || previous.generation !== next.generation
      || previous.developerTools !== next.developerTools || previous.developerLoaded !== next.developerLoaded)
      owner.reconcile();
  }
  if (!listening && next.connected) {
    listening = true;
    void (window as any).__TAURI__.event.listen('acp-event', ({ payload }: any) => {
      if (payload.kind !== 'agent_presets_changed' && payload.kind !== 'client_preferences_changed') return;
      if (view?.visible) void owner?.load();
      owner?.reconcile();
    });
  }
  if (!holder && !next.visible) return;
  if (!holder) {
    holder = document.createElement('div'); holder.id = 'settings-agent-presets';
    holder.style.cssText = 'position:fixed;z-index:2;overflow:auto;overscroll-behavior:contain;pointer-events:auto;box-sizing:border-box;font-family:var(--dsw-font-family);color:var(--dsw-alias-label-primary);font-size:14px;line-height:22px;--dsw-focus-ring-width:2px;';
    document.getElementById('settings-shell')!.append(holder); root = createRoot(holder);
  }
  holder.hidden = !next.visible;
  if (next.visible && owner) {
    const current = owner;
    root.render(<AgentPresetSection key={current.id} t={t} useAgentPresetSection={current.useSnapshot}
      useDeveloperTools={select => select(next.developerTools)} load={current.load}
      view={current.read} closeView={current.close} makeDefault={current.save}
      startCreatorDraft={() => dispatch?.('settings-agent-preset-creator', '')}
      close={() => dispatch?.('settings-close', '')} />);
  } else root.render(next.visible ? <p role="status">连接服务后可查看 Agent 预设。</p> : null);
  if (bounds) placeSettingsAgentPresets(...bounds as [number, number, number, number]);
}
