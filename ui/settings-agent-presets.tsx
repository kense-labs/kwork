/** Official preset settings over the current Host connection. */
import { useSyncExternalStore } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { AgentPresetSection } from './vendor/harness-agent-presets/AgentPresetSection';
import { AgentPresetSectionController } from './vendor/harness-agent-presets/section-store';
import { zh } from './vendor/harness-agent-presets/locales';
import { zh as common } from './vendor/harness-locale/zh';
import { officialModelsEnabled } from './settings-models';

type View = { visible: boolean; connected: boolean; generation: number; locked: boolean; developerTools: boolean };
let view: View;
let holder: HTMLDivElement;
let root: Root;
let bounds: number[];
let dispatch: (action: string, value: string) => void;
let owner: ReturnType<typeof createOwner> | undefined;
let instance = 0;
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
  const reconcile = () => controller.reconcileCodingTools(() => lifetime.active && view?.developerTools === false);
  const load = async () => { await controller.load(); await reconcile(); };
  return { lifetime, controller, load, reconcile, id: ++instance,
    useSnapshot: (select: (value: any) => any) => select(useSyncExternalStore(controller.store.subscribe, controller.store.getSnapshot)),
    read: (id: string) => controller.view(id), close: () => controller.closeView(),
    save: (id: string) => controller.makeDefault(id),
  };
}

export function listenSettingsAgentPresets(callback: typeof dispatch) { dispatch = callback; }
export function placeSettingsAgentPresets(left: number, top: number, width: number, height: number) {
  bounds = [left, top, width, height];
  if (holder) for (const [index, key] of ['left', 'top', 'width', 'height'].entries()) holder.style[key as any] = `${bounds[index]}px`;
}
export function updateSettingsAgentPresets(next: View) {
  if (!officialModelsEnabled()) return;
  const previous = view; view = next;
  if (!holder && !next.visible) return;
  if (!holder) {
    holder = document.createElement('div'); holder.id = 'settings-agent-presets';
    holder.style.cssText = 'position:fixed;z-index:2;overflow:auto;overscroll-behavior:contain;pointer-events:auto;box-sizing:border-box;font-family:var(--dsw-font-family);color:var(--dsw-alias-label-primary);font-size:14px;line-height:22px;--dsw-focus-ring-width:2px;';
    document.getElementById('settings-shell')!.append(holder); root = createRoot(holder);
    void (window as any).__TAURI__.event.listen('acp-event', ({ payload }: any) => {
      if (payload.kind === 'agent_presets_changed' || payload.kind === 'client_preferences_changed') void owner?.load();
    });
  }
  holder.hidden = !next.visible;
  if (!next.visible || !next.connected || previous?.generation !== next.generation || previous?.connected !== next.connected) {
    if (owner) owner.lifetime.active = false;
    owner = undefined; root.render(null);
  }
  if (next.visible && next.connected) {
    owner ??= createOwner();
    const current = owner;
    root.render(<AgentPresetSection key={current.id} t={t} useAgentPresetSection={current.useSnapshot}
      useDeveloperTools={select => select(next.developerTools)} load={current.load}
      view={current.read} closeView={current.close} makeDefault={current.save}
      close={() => dispatch?.('settings-close', '')} />);
    if (previous?.developerTools !== next.developerTools) void current.reconcile();
  } else if (next.visible) root.render(<p role="status">连接服务后可查看 Agent 预设。</p>);
  if (bounds) placeSettingsAgentPresets(...bounds as [number, number, number, number]);
}
