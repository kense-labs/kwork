// Native controls share the painted composer's submission policy and settings.
let dispatch;
let preference;
let submit;
let controls;
let focusSettings = false;
let attachmentControls;
const modes = [['queue', '排队发送'], ['steer', '插话发送']];

export function listen_composer_accessibility(callback) {
  dispatch = callback;
}

function act(action, value = '') {
  queueMicrotask(() => dispatch?.(action, value));
}

function button(label, onClick) {
  const element = document.createElement('button');
  element.type = 'button';
  element.textContent = label;
  element.onclick = onClick;
  return element;
}

function createAttachmentControls() {
  const root = document.createElement('section');
  root.id = 'attachment-accessibility';
  root.className = 'workspace-accessibility-surface';
  const group = label => {
    const element = document.createElement('div');
    element.setAttribute('role', 'group');
    element.setAttribute('aria-label', label);
    root.append(element);
    return { element, buttons: new Map() };
  };
  const drafts = group('草稿附件');
  const files = group('会话文件');
  document.body.append(root);
  return { root, drafts, files };
}

function updateAttachmentGroup(group, items, session, action) {
  const { element, buttons } = group;
  const ids = new Set(items.map(item => item.id));
  const focused = buttons.size > 0 && element.contains(document.activeElement)
    ? document.activeElement : null;
  for (const [id, node] of buttons) {
    if (ids.has(id)) continue;
    node.remove();
    buttons.delete(id);
  }
  items.forEach(({ id, label, size }, index) => {
    let node = buttons.get(id);
    if (!node) {
      node = button(label, null);
      buttons.set(id, node);
    }
    node.textContent = label;
    node.setAttribute('aria-description', size);
    node.onclick = () => act(action, JSON.stringify({ session, id }));
    const current = element.children[index];
    if (current !== node) element.insertBefore(node, current ?? null);
  });
  element.hidden = items.length === 0;
  if (focused && !focused.isConnected) {
    const fallback = element.firstElementChild ?? controls?.primary;
    if (fallback && !fallback.hidden && !fallback.disabled) fallback.focus({ preventScroll: true });
  }
}

export function update_attachment_accessibility(view) {
  if (!view && !attachmentControls) return;
  const { root, drafts, files } = attachmentControls ??= createAttachmentControls();
  updateAttachmentGroup(drafts, view?.drafts ?? [], view?.session, 'attachment-remove');
  updateAttachmentGroup(files, view?.files ?? [], view?.session, 'attachment-download');
  root.hidden = drafts.element.hidden && files.element.hidden;
}

function create() {
  const root = document.createElement('section');
  root.id = 'composer-submit-accessibility';
  root.className = 'workspace-accessibility-surface';
  const primary = button('发送', () => {
    if (submit?.enabled) act('composer-submit', submit.label);
  });
  const more = button('更多设置', () => act('composer-more-settings'));
  const openSettings = button('设置', () => act('composer-open-settings'));
  more.setAttribute('aria-controls', 'accessible-open-settings');
  openSettings.id = 'accessible-open-settings';
  const general = button('通用设置', () => {
    focusSettings = true;
    act('composer-general-settings');
  });
  const providers = button('模型服务', () => act('composer-model-providers'));
  const account = button('DeepSeek 账号', () => act('composer-account-settings'));
  const settings = document.createElement('div');
  settings.setAttribute('role', 'group');
  settings.setAttribute('aria-label', '繁忙时的发送行为');
  const trigger = button('', () => {
    if (preference && !preference.busy) setOpen(menu.hidden);
  });
  trigger.setAttribute('aria-haspopup', 'menu');
  trigger.setAttribute('aria-controls', 'accessible-busy-enter-options');
  const menu = document.createElement('div');
  menu.id = 'accessible-busy-enter-options';
  menu.setAttribute('role', 'menu');
  menu.hidden = true;

  const options = new Map(modes.map(([mode, label]) => {
    const option = button(label, () => {
      setOpen(false);
      act('composer-preference', mode);
    });
    option.setAttribute('role', 'menuitemradio');
    menu.append(option);
    return [mode, option];
  }));
  function setOpen(open) {
    menu.hidden = !open;
    trigger.setAttribute('aria-expanded', String(open));
    if (open) options.get(preference.mode)?.focus({ preventScroll: true });
    else if (settings.contains(document.activeElement)) trigger.focus({ preventScroll: true });
  }
  settings.addEventListener('keydown', event => {
    if (!preference || event.isComposing) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      if (menu.hidden) act('composer-preference-close');
      else setOpen(false);
    } else if (!preference.busy && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
      event.preventDefault();
      if (menu.hidden) setOpen(true);
      else {
        const items = [...options.values()];
        const selected = items.indexOf(document.activeElement);
        items[(selected + 1) % items.length].focus({ preventScroll: true });
      }
    }
  });
  settings.addEventListener('focusout', event => {
    if (event.relatedTarget && !settings.contains(event.relatedTarget)) {
      menu.hidden = true;
      trigger.setAttribute('aria-expanded', 'false');
    }
  });
  const alert = document.createElement('div');
  alert.setAttribute('role', 'alert');
  const retry = button('重试读取发送设置', () => act('composer-preference'));
  settings.append(trigger, menu, alert, retry);
  root.append(primary, more, openSettings, general, providers, account, settings);
  document.body.append(root);
  return { root, primary, more, openSettings, general, providers, account, settings, trigger, menu, options, alert, retry };
}

export function update_composer_accessibility(navigation, nextPreference, nextSubmit) {
  const settingsOpen = navigation.settings_open;
  preference = nextPreference;
  submit = nextSubmit;
  if (!settingsOpen && navigation.more_open == null && !submit && !controls) return;
  controls ??= create();
  const { root, primary, more, openSettings, general, providers, account, settings, trigger, menu, options, alert, retry } = controls;
  root.hidden = !submit && !settingsOpen && navigation.more_open == null;
  more.hidden = navigation.more_open == null;
  more.setAttribute('aria-expanded', String(navigation.more_open === true));
  openSettings.hidden = navigation.more_open !== true;
  general.hidden = !settingsOpen || Boolean(preference);
  providers.hidden = !settingsOpen || navigation.providers_open;
  account.hidden = !settingsOpen || !navigation.account_available || navigation.account_open;
  primary.hidden = !submit;
  primary.textContent = submit?.label ?? '发送';
  primary.disabled = !submit?.enabled;
  settings.hidden = !preference;
  if (!preference) {
    if (!settingsOpen) focusSettings = false;
    menu.hidden = true;
    trigger.setAttribute('aria-expanded', 'false');
    return;
  }
  const label = modes.find(([mode]) => mode === preference.mode)?.[1] ?? modes[0][1];
  trigger.textContent = label;
  trigger.setAttribute('aria-label', `繁忙时的发送行为，${label}`);
  if (!trigger.hasAttribute('aria-expanded')) trigger.setAttribute('aria-expanded', 'false');
  // Keep the trigger focused during the write so users can see the accepted value.
  trigger.setAttribute('aria-busy', String(preference.busy));
  trigger.setAttribute('aria-disabled', String(preference.busy));
  for (const [mode, option] of options) {
    option.setAttribute('aria-checked', String(mode === preference.mode));
    option.disabled = preference.busy;
  }
  alert.hidden = retry.hidden = !preference.error;
  alert.textContent = preference.error ?? '';
  retry.disabled = preference.busy;
  if (focusSettings) {
    focusSettings = false;
    trigger.focus({ preventScroll: true });
  }
}
