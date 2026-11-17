import { listenSettingsDeveloper, updateSettingsDeveloper, listenSettingsPermission, updateSettingsPermission, officialModelsEnabled, updateSettingsModels, updateSettingsPlugins, listenSettingsUpload, updateSettingsUpload, focusWithoutRing, listenSettingsTheme, updateSettingsTheme, placeSettingsThemeViewport } from '/trajectory-json.js';
import { listenClientPreferences, updateClientPreferencesConnection, refreshClientPreferences } from '/trajectory-json.js';
import { listenSettingsAgentPresets, updateSettingsAgentPresets } from '/trajectory-json.js';
export { placeSettingsAgentPresets as place_settings_agent_presets } from '/trajectory-json.js';
export { saveClientTheme as save_client_theme } from '/trajectory-json.js';
export { placeSettingsTheme as place_settings_theme, settingsThemeHeight as settings_theme_height } from '/trajectory-json.js';
export { placeSettingsPlugins as place_settings_plugins } from '/trajectory-json.js';
export { placeSettingsDeveloper as place_settings_developer, settingsDeveloperHeight as settings_developer_height } from '/trajectory-json.js';
export { placeSettingsPermission as place_settings_permission, settingsPermissionHeight as settings_permission_height } from '/trajectory-json.js';
export { placeSettingsUpload as place_settings_upload, settingsUploadHeight as settings_upload_height } from '/trajectory-json.js';

// SettingsRoot / SettingsDocumentAction chrome. Page content stays owned by GPUI.
let dispatch;
let controls;
let view;
let bounds;
let documentState;
let returnFocus;
let lastShellFocus;
let modalReturnFocus;
let modalCovered = false;
const pages = [
  ['账号与余额', 'user-outline'], ['API Key', 'api-outline'],
  ['通用设置', 'settings-outline'],
  ['网页搜索', 'search-outline'], ['模型', 'data-outline'],
  ['模型服务', 'globe-outline'], ['内置插件', 'layers-outline'],
  ['Agent 预设', 'agent-preset-outline'], ['会话日志', 'archive-outline'],
];
const invoke = operation => Promise.resolve().then(() =>
  window.__TAURI__.core.invoke('settings_document', { operation }));
const act = (action, value = '') => queueMicrotask(() => dispatch?.(action, value));

export function listen_settings_shell(callback) { dispatch = callback; listenSettingsTheme(callback); listenSettingsUpload(callback); listenSettingsPermission(callback); listenSettingsDeveloper(callback); listenClientPreferences(callback); listenSettingsAgentPresets(callback); }
export function place_settings_shell(left, top, width, height) {
  bounds = { left, top, width, height };
  place();
}
function place() {
  if (!controls || !bounds) return;
  for (const [key, value] of Object.entries(bounds)) controls.root.style[key] = `${value}px`;
  // The scrollport is fixed in the panel. Measuring an absolute child of the
  // GPUI scroller would move this clipping boundary along with its content.
  placeSettingsThemeViewport(bounds.left + 188, bounds.top + 54, bounds.width - 188, bounds.height - 54);
}
function icon(name, size = 16, weight = 'medium') {
  return `<svg width="${size}" height="${size}" aria-hidden="true"><use href="#dsw-icon-${name}-${weight}" /></svg>`;
}
function mount() {
  const root = document.createElement('section');
  root.id = 'settings-shell';
  root.setAttribute('role', 'dialog');
  root.setAttribute('aria-modal', 'true');
  root.setAttribute('aria-label', '设置');
  root.innerHTML = `<div class="settings-title">设置</div>
    <nav aria-label="设置页面"><div class="settings-nav-list"></div></nav>
    <header><div class="settings-actions">
      <div class="settings-document" hidden><span role="alert" hidden>无法打开配置文件</span>
        <button type="button" class="settings-open-document">打开配置文件</button></div>
      <button type="button" class="settings-close" aria-label="关闭设置">${icon('close-outline', 14, 'regular')}</button>
    </div></header>`;
  const nav = root.querySelector('.settings-nav-list');
  const buttons = pages.map(([label, glyph], index) => {
    if (officialModelsEnabled() && index === 5) { label = '模型'; glyph = 'data-outline'; }
    if (officialModelsEnabled() && index === 6) glyph = 'personalization-outline';
    const button = document.createElement('button');
    button.type = 'button';
    button.innerHTML = `${icon(glyph)}<span>${label}</span>`;
    button.onclick = () => { if (!view?.navigation_locked) act('settings-page', String(index)); };
    button.onfocus = () => button.scrollIntoView({ block: 'nearest' });
    nav.append(button);
    return button;
  });
  controls = { root, buttons, action: root.querySelector('.settings-document'),
    error: root.querySelector('[role="alert"]'), open: root.querySelector('.settings-open-document'),
    close: root.querySelector('.settings-close') };
  controls.open.onclick = openDocument;
  controls.close.onclick = () => act('settings-close');
  root.addEventListener('keydown', event => {
    if (event.isComposing || event.defaultPrevented || !view?.visible || view.locked) return;
    if (event.key === 'Escape') {
      event.preventDefault(); event.stopPropagation();
      if (!event.repeat) act('settings-close');
    }
  });
  document.addEventListener('keydown', event => {
    if (event.defaultPrevented || event.isComposing || event.key !== 'Tab' || !view?.visible || view.locked) return;
    // Provider/account forms live in sibling portals; the General theme rows
    // are actual descendants. All three keep their native Tab boundaries in
    // the shared dialog while nested pickers keep their own traversal.
    if (![0, 2, 5].includes(view.page) && !([6, 7].includes(view.page) && officialModelsEnabled())) return;
    const foreground = [...document.querySelectorAll('[role="dialog"][aria-modal="true"]')].filter(node => !node.hidden && node.getClientRects().length).at(-1);
    if (foreground && foreground !== root) return;
    const content = view.page === 2 || [5, 6, 7].includes(view.page) && officialModelsEnabled() ? null : document.getElementById(view.page === 5 ? 'model-provider-form' : 'deepseek-account-form');
    const candidates = holder => [...(holder?.querySelectorAll('button, input, select, textarea, a[href], summary, [tabindex="0"]') ?? [])]
      .filter(node => !node.disabled && node.getClientRects().length && !node.closest('[hidden], [inert], [aria-hidden="true"]'));
    const chrome = candidates(root), fields = candidates(content);
    const current = document.activeElement;
    let target;
    if (event.shiftKey && current === chrome[0]) target = fields.at(-1) ?? chrome.at(-1);
    else if (!event.shiftKey && current === chrome.at(-1)) target = fields[0] ?? chrome[0];
    else if (event.shiftKey && current === fields[0]) target = chrome.at(-1);
    else if (!event.shiftKey && current === fields.at(-1)) target = chrome[0];
    if (target) { event.preventDefault(); target.focus({ preventScroll: true }); }
  });
  root.addEventListener('focusin', event => {
    if (event.target instanceof HTMLElement) lastShellFocus = event.target;
  });
  new MutationObserver(syncModalOwnership).observe(document.body, { childList: true });
  document.body.append(root);
  place();
}
function syncModalOwnership() {
  if (!controls || !view?.visible) return;
  const shell = controls.root;
  if (!shell) return;
  const foreground = [...document.querySelectorAll('[role="dialog"][aria-modal="true"]')]
    .filter(element => !element.hidden && element.getClientRects().length).at(-1);
  const covered = !!foreground && foreground !== shell;
  if (covered && !modalCovered) modalReturnFocus = lastShellFocus;
  shell.inert = view.locked || covered;
  shell.setAttribute('aria-hidden', String(view.locked || covered));
  // React's modal cleanup runs before MutationObserver clears our native
  // shell's inert flag. Restore the saved control after releasing that flag.
  if (!covered && modalCovered && !view.locked) {
    if (document.activeElement === document.body && modalReturnFocus?.isConnected
      && !modalReturnFocus.closest('[inert], [hidden]')) focusWithoutRing(modalReturnFocus);
    modalReturnFocus = undefined;
  }
  modalCovered = covered;
}
function paintDocument() {
  if (!controls) return;
  controls.action.hidden = !documentState?.available;
  controls.open.disabled = !!documentState?.opening;
  controls.error.hidden = !documentState?.error;
}
async function loadDocument(owner) {
  owner.loading = true;
  try {
    const reply = await invoke('describe');
    if (documentState !== owner) return;
    owner.available = reply?.available === true;
  } catch { /* Upstream hides the action when metadata cannot be read. */ }
  finally {
    if (documentState === owner) { owner.loading = false; owner.loaded = true; paintDocument(); }
  }
}
async function openDocument() {
  const owner = documentState;
  if (!view?.visible || view.locked || !owner?.available || owner.opening) return;
  owner.opening = true; owner.error = false; paintDocument();
  try {
    const reply = await invoke('open');
    if (documentState === owner && reply?.opened !== true) owner.error = true;
  } catch { if (documentState === owner) owner.error = true; }
  finally { if (documentState === owner) { owner.opening = false; paintDocument(); } }
}
export function update_settings_shell(next) {
  const previous = view;
  view = next;
  if (officialModelsEnabled()) updateClientPreferencesConnection(next.connected);
  if (officialModelsEnabled() && next.visible && !previous?.visible) refreshClientPreferences();
  // Token sheets use the body attribute; synchronize even before a trajectory
  // has rendered, so settings opened directly from Chat inherits its theme.
  document.body.toggleAttribute('data-ds-dark-theme', next.dark);
  if (!previous || previous.generation !== next.generation || previous.connected !== next.connected) {
    documentState = next.connected ? {} : undefined;
  }
  if (next.visible && !controls) mount();
  updateSettingsTheme({ visible: next.visible && next.page === 2, locked: next.locked || officialModelsEnabled() && !next.theme_writable,
    preference: next.theme, size: next.font_size, fonts: next.fonts });
  updateSettingsAgentPresets({ visible: next.visible && next.page === 7, connected: next.connected,
    generation: next.generation, locked: next.locked, developerTools: next.developer_tools,
    developerLoaded: officialModelsEnabled() ? next.developer_loaded : !next.developer_loading,
    blankSession: next.blank_preset_session });
  if (!controls) return;
  const { root, buttons } = controls;
  root.hidden = !next.visible;
  root.inert = next.locked;
  root.setAttribute('aria-hidden', String(next.locked));
  root.setAttribute('aria-owns', next.locked ? '' : next.page === 5 && !officialModelsEnabled() ? 'model-provider-form' : next.page === 0 ? 'deepseek-account-form' : '');
  updateSettingsModels({ visible: next.visible && next.page === 5, connected: next.connected, generation: next.generation, locked: next.locked });
  updateSettingsPlugins({ visible: next.visible && next.page === 6, connected: next.connected, generation: next.generation, locked: next.locked });
  if (officialModelsEnabled()) updateSettingsDeveloper({ visible: next.visible && next.page === 2, locked: next.locked || !next.developer_writable, enabled: next.developer_tools, loading: next.developer_loading });
  if (officialModelsEnabled()) updateSettingsPermission({ visible: next.visible && next.page === 2, connected: next.connected, generation: next.generation, locked: next.locked });
  if (officialModelsEnabled()) updateSettingsUpload({ visible: next.visible && next.page === 2, connected: next.connected, generation: next.generation, locked: next.locked });
  buttons.forEach((button, index) => {
    button.hidden = index === 0 && !next.account || officialModelsEnabled() && [1, 3, 4, 8].includes(index);
    button.disabled = next.navigation_locked;
    if (index === next.page) button.setAttribute('aria-current', 'true');
    else button.removeAttribute('aria-current');
  });
  if (next.visible && !previous?.visible && documentState && !documentState.loading) {
    documentState.available = false;
    documentState.loaded = false;
    documentState.error = false;
  }
  paintDocument();
  syncModalOwnership();
  if (next.visible && documentState && !documentState.loading && !documentState.loaded) void loadDocument(documentState);
  if (next.visible && !previous?.visible) {
    returnFocus = document.activeElement;
    requestAnimationFrame(() => {
      if (view?.visible && !view.locked && buttons[view.page]) focusWithoutRing(buttons[view.page], { preventScroll: true });
    });
  } else if (!next.visible && previous?.visible) {
    if (returnFocus?.isConnected && !returnFocus.closest('[hidden], [inert]')) focusWithoutRing(returnFocus, { preventScroll: true });
    returnFocus = undefined;
  }
}
