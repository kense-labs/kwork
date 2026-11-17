/** Official future-session permission row, measured into the native settings scrollport. */
import { useSyncExternalStore } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { MeasuredSettingsRow } from './settings-native-row';
import { createPermissionController } from './settings-permission-controller';
import { PermissionRow } from './vendor/harness-settings-permission/PermissionRow';
import { zh } from './vendor/harness-settings-permission/locales';

type View = { visible: boolean; locked: boolean; connected: boolean; generation: number };
let view: View;
let row: MeasuredSettingsRow;
let root: Root;
let owner: ReturnType<typeof createPermissionController> | undefined;
let bounds: number[];
let dispatch: (action: string, value: string) => void;
export function listenSettingsPermission(callback: typeof dispatch) { dispatch = callback; }
export function settingsPermissionHeight() { return row?.height ?? 0; }
export function placeSettingsPermission(left: number, top: number, width: number) {
  bounds = [left, top, width]; row?.measure(left, top, width);
}
function Content({ scope }: { scope: ReturnType<typeof createPermissionController> }) {
  return <PermissionRow load={scope.load} select={scope.select}
    usePermission={select => select(useSyncExternalStore(scope.controller.store.subscribe, scope.controller.store.getSnapshot))}
    t={key => key === 'close' ? '关闭' : zh[key]} />;
}
export function updateSettingsPermission(next: View) {
  const previous = view; view = next;
  if (!row && !next.visible) return;
  if (!row) {
    row = new MeasuredSettingsRow('settings-permission', 0, () => dispatch?.('settings-permission-layout', ''));
    root = createRoot(row.element);
    void (window as any).__TAURI__.event.listen('acp-event', ({ payload }: any) => {
      if (payload?.kind === 'permission_default_changed') void owner?.load();
    });
  }
  row.show(next.visible, next.locked);
  if (!next.visible || !next.connected || previous?.generation !== next.generation) {
    owner?.dispose(); owner = undefined; root.render(null);
  }
  if (next.visible && next.connected && !owner) {
    owner = createPermissionController(request => (window as any).__TAURI__.core.invoke('permission_default', { request }));
    root.render(<Content scope={owner} />);
  }
  if (bounds) row.measure(...bounds as [number, number, number]);
}
