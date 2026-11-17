import { createClientPreferencesController } from './client-preferences-controller';

let owner: ReturnType<typeof createClientPreferencesController> | undefined;
let connected = false;
let listening = false;
let dispatch: (action: string, value: string) => void;
export function listenClientPreferences(callback: typeof dispatch) { dispatch = callback; }
const send = (action: string, value: unknown) => queueMicrotask(() => dispatch?.(action, typeof value === 'string' ? value : JSON.stringify(value)));
export function updateClientPreferencesConnection(next: boolean) {
  if (!listening) {
    listening = true;
    void (window as any).__TAURI__.event.listen('acp-event', ({ payload }: any) => {
      if (payload?.kind === 'client_preferences_changed') void owner?.load().catch(() => {});
    });
  }
  if (next === connected) return;
  connected = next;
  owner?.dispose(); owner = undefined;
  send('settings-client-preferences', { writable: false, available: [], namespaces: [] });
  if (next) {
    const current = createClientPreferencesController(request => (window as any).__TAURI__.core.invoke('client_preferences', { request }),
      snapshot => queueMicrotask(() => {
        if (owner === current) dispatch?.('settings-client-preferences', JSON.stringify(snapshot));
      }));
    owner = current;
    void current.load().catch(() => { if (owner === current) send('settings-client-preferences-error', '读取界面偏好失败。'); });
  }
}
export function saveClientPreference(namespace: string, changes: Record<string, unknown>) {
  return owner?.set(namespace, changes) ?? Promise.reject(new Error('Host is disconnected'));
}
export function refreshClientPreferences() { void owner?.load().catch(() => {}); }
export function saveClientTheme(changes: Record<string, unknown>) {
  const current = owner;
  void saveClientPreference('ui-theme', changes).catch(() => {
    if (owner === current) send('settings-client-preferences-error', '界面偏好未保存，已恢复已保存的设置。');
  });
}
