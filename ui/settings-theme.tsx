/** Measured GPUI adapter for the official General appearance/text-size rows. */
import { createRoot, type Root } from 'react-dom/client';
import { AppearanceRow } from './vendor/harness-settings-theme/AppearanceRow';
import { MeasuredSettingsRow, placeSettingsRowsViewport } from './settings-native-row';
import { FontSettingsGroup } from './vendor/harness-settings-theme/FontSettingsGroup';
import { zh } from './vendor/harness-settings-theme/locales';
import { normalizeFontFamily } from './vendor/harness-settings-theme/font-size-specs';

type View = { visible: boolean; locked: boolean; preference: string; size: number; fonts: { codeFontSize: number; terminalFontSize: number; textFontFamily: string; codeFontFamily: string; terminalFontFamily: string } };
let view: View;
let row: MeasuredSettingsRow;
let root: Root;
let bounds: number[];
let dispatch: (action: string, value: string) => void;
export function listenSettingsTheme(callback: typeof dispatch) { dispatch = callback; }
export function settingsThemeHeight() { return row?.height ?? 221; }
const act = (action: string, value: string) => {
  if (view?.visible && !view.locked) queueMicrotask(() => dispatch?.(action, value));
};
export function placeSettingsTheme(left: number, top: number, width: number) {
  bounds = [left, top, width]; row?.measure(left, top, width);
}
export const placeSettingsThemeViewport = placeSettingsRowsViewport;
export function updateSettingsTheme(next: View) {
  view = next;
  const fonts = next.fonts;
  const fontSizes = { text: next.size, code: fonts.codeFontSize, terminal: fonts.terminalFontSize };
  const fontFamilies = { text: fonts.textFontFamily, code: fonts.codeFontFamily, terminal: fonts.terminalFontFamily };
  for (const role of ['text', 'code', 'terminal'] as const) {
    const variable = role === 'text' ? '--dsh-content-font-size' : `--dsh-${role}-font-size`;
    document.body.style.setProperty(variable, `${fontSizes[role]}px`);
    const family = normalizeFontFamily(fontFamilies[role]);
    if (family) document.body.style.setProperty(`--dsh-font-family-${role}`, family);
    else document.body.style.removeProperty(`--dsh-font-family-${role}`);
  }
  if (!row && !next.visible) return;
  if (!row) {
    row = new MeasuredSettingsRow('settings-theme', 221, () => dispatch?.('settings-theme-layout', ''));
    root = createRoot(row.element);
  }
  row.show(next.visible, next.locked);
  root.render(next.visible ? <>
    <AppearanceRow t={key => zh[key]} useStore={select => select({ preference: next.preference })}
      setTheme={preference => act('settings-theme', preference)} />
    <FontSettingsGroup t={key => zh[key]} useStore={select => select({ fontSizes, fontFamilies })}
      setFontFamily={(role, family) => act('settings-font-family', JSON.stringify([role, family]))}
      // Dispatch relative steps: GPUI publishes only after its next paint.
      setFontSize={(role, size) => act('settings-font-role-step', JSON.stringify([role, size > fontSizes[role] ? 1 : -1]))} />
  </> : null);
  if (bounds) row.measure(...bounds as [number, number, number]);
}
