/** Official General upload row; the accepted value and notice belong to the connection. */
import { useSyncExternalStore } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { MeasuredSettingsRow } from './settings-native-row';
import { createUploadController } from './settings-upload-controller';
import { UploadRow, UploadToast } from './vendor/harness-settings-upload/UploadRow';
import { zh } from './vendor/harness-settings-upload/locales';

type View = { visible: boolean; locked: boolean; connected: boolean; generation: number };
let view: View;
let row: MeasuredSettingsRow;
let root: Root;
let owner: ReturnType<typeof createUploadController> | undefined;
let dispatch: (action: string, value: string) => void;
let bounds: number[];
let ownerId = 0;
export function listenSettingsUpload(callback: typeof dispatch) { dispatch = callback; }
export function settingsUploadHeight() { return row?.height ?? 0; }
export function placeSettingsUpload(left: number, top: number, width: number) {
  bounds = [left, top, width]; row?.measure(left, top, width);
}
function Content({ controller }: { controller: ReturnType<typeof createUploadController> }) {
  const snapshot = useSyncExternalStore(controller.form.subscribe, controller.form.getSnapshot);
  const face = {
    useUpload: (select: (value: any) => any) => select(useSyncExternalStore(controller.form.subscribe, controller.form.getSnapshot)),
    useMutation: (select: (value: any) => any) => select(useSyncExternalStore(controller.preference.state.subscribe, controller.preference.state.getSnapshot)),
    setEnabled: (enabled: boolean) => controller.preference.setEnabled(enabled),
    dismiss: () => controller.preference.dismiss(), t: (key: keyof typeof zh) => zh[key],
  };
  return <>{snapshot.available && <UploadRow {...face} />}<UploadToast {...face} /></>;
}
export function updateSettingsUpload(next: View) {
  const previous = view; view = next;
  if (!row && !next.visible) return;
  if (!row) {
    row = new MeasuredSettingsRow('settings-upload', 0, () => dispatch?.('settings-upload-layout', ''));
    root = createRoot(row.element);
    void (window as any).__TAURI__.event.listen('acp-event', ({ payload }: any) => {
      if (payload?.kind === 'session_log_preference_changed') void owner?.load();
    });
  }
  row.show(next.visible, next.locked);
  if (!next.connected || previous?.generation !== next.generation) {
    owner?.dispose(); owner = undefined; root.render(null);
  }
  if (next.connected && !owner) {
    owner = createUploadController(request => (window as any).__TAURI__.core.invoke('session_log_preference', { request }));
    root.render(<Content key={++ownerId} controller={owner} />);
    void owner.load();
  } else if (next.visible && !previous?.visible) void owner?.load();
  if (bounds) row.measure(...bounds as [number, number, number]);
}
