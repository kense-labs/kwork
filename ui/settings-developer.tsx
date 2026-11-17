/** Official coding-view row over the Host's accepted shared preference. */
import { createRoot, type Root } from 'react-dom/client';
import { MeasuredSettingsRow } from './settings-native-row';
import { DeveloperToolsRow } from './vendor/harness-settings-developer/DeveloperToolsRow';
import { zh } from './vendor/harness-settings-developer/locales';
import { saveClientPreference } from './client-preferences';

type View = { visible: boolean; locked: boolean; enabled: boolean; loading: boolean };
let row: MeasuredSettingsRow;
let root: Root;
let bounds: number[];
let dispatch: (action: string, value: string) => void;
export function listenSettingsDeveloper(callback: typeof dispatch) { dispatch = callback; }
export function settingsDeveloperHeight() { return row?.height ?? 76; }
export function placeSettingsDeveloper(left: number, top: number, width: number) {
  bounds = [left, top, width]; row?.measure(left, top, width);
}
// The shared connection owner outlives the row and orders accepted Host writes.
const setEnabled = (enabled: boolean) => saveClientPreference('ui-settings', { enabled });
export function updateSettingsDeveloper(next: View) {
  if (!row && !next.visible) return;
  if (!row) {
    row = new MeasuredSettingsRow('settings-developer', 76, () => dispatch?.('settings-developer-layout', ''));
    root = createRoot(row.element);
  }
  row.show(next.visible, next.locked || next.loading);
  root.render(next.visible ? <DeveloperToolsRow setEnabled={setEnabled}
    useDeveloperTools={select => select(next.enabled)} t={key => zh[key]} /> : null);
  if (bounds) row.measure(...bounds as [number, number, number]);
}
