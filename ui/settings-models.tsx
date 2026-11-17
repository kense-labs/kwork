/** Official Models cards, backed by the existing native ACP connection. */
import { useSyncExternalStore } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { ModelsSection } from './vendor/harness-settings-models/ModelsSection';
import { ModelsSettingsStore } from './vendor/harness-settings-models/store';
import { createModelsOperations } from './vendor/harness-settings-models/operations';
import { SettingsSchemaService } from './vendor/harness-settings-models/schema';
import { zh } from './vendor/harness-settings-models/locales';

type View = { visible: boolean; connected: boolean; generation: number; locked: boolean };
type Reply = { ok: true; value: any } | { ok: false; error: { code: string; message: string } };
let view: View;
let holder: HTMLDivElement;
let root: Root;
let bounds: number[];
let owner: ReturnType<typeof createController> | undefined;
let instance = 0;
const schema = new SettingsSchemaService();
const native = () => (window as any).__TAURI__;
export const officialModelsEnabled = () => document.documentElement.dataset.platform === 'darwin';

function createController() {
  const lifetime = { active: true };
  const call = async (method: string, args: unknown[] = []): Promise<Reply> => {
    if (!lifetime.active) return { ok: false, error: { code: 'connection/changed', message: '服务连接已变化，请重试。' } };
    try {
      const reply = await native().core.invoke('models_settings', { request: { method, args } });
      if (!lifetime.active) throw new Error('服务连接已变化，请重试。');
      return reply;
    } catch (error) {
      return { ok: false, error: { code: 'connection/unavailable', message: error instanceof Error ? error.message : String(error) } };
    }
  };
  const remote = {
    llm: { listProviders: () => call('llm/listProviders'), listConfigurableProviders: () => call('llm/listConfigurableProviders'),
      discoverModels: (ns: string, request: unknown) => call('llm/discoverModels', [ns, request]) },
    credentials: { describe: (refs: string[]) => call('credentials/describe', [refs]),
      set: (ref: string, value: string) => call('credentials/set', [ref, value]), unset: (ref: string) => call('credentials/unset', [ref]) },
    settings: { mutate: (ns: string, ops: unknown[], revision?: number) => call('settings/mutate', [ns, ops, revision]) },
    session: { modelCatalog: () => call('session/modelCatalog') },
  };
  let mirrored: { view?: any; error: string | null } = { error: null };
  let generation = 0;
  const describe = {
    getSnapshot: () => mirrored,
    ensure: async () => {
      const current = ++generation;
      const response = await call('settings/describe');
      if (current === generation) mirrored = response.ok ? { view: response.value, error: null }
        : { ...mirrored, error: response.error.message };
    },
  };
  const controller = new ModelsSettingsStore({ remote } as any, schema, describe as any);
  const operations = createModelsOperations({ remote } as any);
  const useSnapshot = (select: (value: any) => any) => select(useSyncExternalStore(controller.store.subscribe, controller.store.getSnapshot));
  return { id: ++instance, lifetime, controller, operations, useSnapshot };
}
export function placeSettingsModels(left: number, top: number, width: number, height: number) {
  bounds = [left, top, width, height];
  if (!holder) return;
  for (const [index, key] of ['left', 'top', 'width', 'height'].entries()) holder.style[key as any] = `${bounds[index]}px`;
}
export function updateSettingsModels(next: View) {
  if (!officialModelsEnabled()) return;
  const previous = view;
  view = next;
  if (!holder && !next.visible) return;
  if (!holder) {
    holder = document.createElement('div'); holder.id = 'settings-models';
    holder.style.cssText = 'position:fixed;z-index:2;overflow:auto;overscroll-behavior:contain;pointer-events:auto;box-sizing:border-box;font-family:var(--dsw-font-family);color:var(--dsw-alias-label-primary);font-size:14px;line-height:22px;--dsw-focus-ring-width:2px;';
    const shell = document.getElementById('settings-shell')!;
    shell.append(holder);
    root = createRoot(holder);
    // The Host emits this after settings/credential/model changes. A reload
    // retains mounted editor drafts; the official store rejects stale reads.
    void native().event.listen('acp-event', ({ payload }: any) => {
      if (payload?.kind === 'model_catalog_changed' && view.visible && owner?.lifetime.active) void owner.controller.load();
    });
  }
  holder.hidden = !next.visible;
  if (!next.visible || !next.connected || previous?.generation !== next.generation || previous?.connected !== next.connected) {
    if (owner) owner.lifetime.active = false;
    owner = undefined;
    root.render(null);
  }
  if (next.visible && next.connected && !owner) {
    owner = createController();
    root.render(<ModelsSection key={owner.id} {...owner} schema={schema} t={key => zh[key]} renderSlot={() => null} />);
    void owner.controller.load();
  } else if (next.visible && !next.connected) root.render(<p role="status">连接服务后可管理模型。</p>);
  if (bounds) placeSettingsModels(...bounds as [number, number, number, number]);
}
