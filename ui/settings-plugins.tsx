/** Official read-only plugin inventory on the current native ACP connection. */
import { createRoot, type Root } from 'react-dom/client';
import { PluginsSettingsSection } from './vendor/harness-settings-plugins/PluginsSettingsSection';
import { PluginInventorySettingsTab } from './vendor/harness-settings-plugins/PluginInventorySettingsTab';
import { zh as sectionCopy } from './vendor/harness-settings-plugins/locales';
import { zh as inventoryCopy } from './vendor/harness-settings-plugins/inventory-locales';
import { zh as presetCopy } from './vendor/harness-agent-presets/locales';
import { presetDisplayText } from './vendor/harness-agent-presets/display';
import { officialModelsEnabled } from './settings-models';

type View = { visible: boolean; connected: boolean; generation: number; locked: boolean };
let view: View;
let holder: HTMLDivElement;
let root: Root;
let bounds: number[];
let owner: { active: boolean; id: number } | undefined;
let instance = 0;
const tabs = [{ id: 'all', order: 10, label: inventoryCopy.tab }];
// KWork's client modules are statically bundled. This is independent of the
// actual Host fiber phases and optional agent preset rows returned by list().
const clientSync = { syncing: false, failures: [] };
const translate = (copy: Record<string, string>) => (key: string, values: Record<string, string> = {}) =>
  copy[key].replace(/\{([^}]+)\}/g, (match, name) => values[name] ?? match);
const inventoryText = translate(inventoryCopy);
const resolveText = (text: string | Record<string, string>) => typeof text === 'string'
  ? text : text.zh ?? text.en ?? Object.values(text)[0] ?? '';

export function placeSettingsPlugins(left: number, top: number, width: number, height: number) {
  bounds = [left, top, width, height];
  if (!holder) return;
  for (const [index, key] of ['left', 'top', 'width', 'height'].entries()) holder.style[key as any] = `${bounds[index]}px`;
}

export function updateSettingsPlugins(next: View) {
  if (!officialModelsEnabled()) return;
  const previous = view;
  view = next;
  if (!holder && !next.visible) return;
  if (!holder) {
    holder = document.createElement('div'); holder.id = 'settings-plugins';
    holder.style.cssText = 'position:fixed;z-index:2;overflow:auto;overscroll-behavior:contain;pointer-events:auto;box-sizing:border-box;font-family:var(--dsw-font-family);color:var(--dsw-alias-label-primary);font-size:14px;line-height:22px;--dsw-focus-ring-width:2px;';
    document.getElementById('settings-shell')!.append(holder);
    root = createRoot(holder);
  }
  holder.hidden = !next.visible;
  if (!next.visible || !next.connected || previous?.generation !== next.generation || previous?.connected !== next.connected) {
    if (owner) owner.active = false;
    owner = undefined;
    root.render(null);
  }
  if (next.visible && next.connected && !owner) {
    const lifetime = owner = { active: true, id: ++instance };
    const list = async () => {
      if (!lifetime.active) throw new Error('Connection changed');
      const snapshot = await (window as any).__TAURI__.core.invoke('plugin_inventory');
      if (!lifetime.active) throw new Error('Connection changed');
      return snapshot;
    };
    root.render(<PluginsSettingsSection key={lifetime.id} t={translate(sectionCopy)}
      useTabs={select => select(tabs)} renderSlot={() => <PluginInventorySettingsTab
        list={list} resolveText={resolveText} t={inventoryText}
        presetName={preset => presetDisplayText(preset, key => presetCopy[key]).name}
        useClientSync={select => select(clientSync)} retryClient={() => {}} />} />);
  } else if (next.visible && !next.connected) root.render(<p role="status">连接服务后可查看插件。</p>);
  if (bounds) placeSettingsPlugins(...bounds as [number, number, number, number]);
}
