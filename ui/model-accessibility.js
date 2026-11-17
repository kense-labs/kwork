// The canvas renders the picker. Native controls expose its state and send
// actions back to the same session, search, and catalog handlers.
let controls;
let dispatch;
let selector;
let teamDisable;
let teamDialogFocus = false;
let permissionRisk;
let riskDialogFocus = false;
let permissionReturnFocus = false;
let moveFocus = false;
let composing = false;
let nativeSearchFocused = false;
let pending;
let commandReturnFocus;

export function listen_model_selector(callback) {
  dispatch = callback;
  const editor = document.getElementById('gpui-text-input');
  if (editor) {
    new MutationObserver(syncEditorAccessibility).observe(editor, { attributes: true, attributeFilter: ['data-gpui-editor-active'] });
    syncEditorAccessibility();
  }
}

function send(action, value = '', focus = false) {
  const pane = selector?.pane?.kind;
  queueMicrotask(() => {
    if (!selector || selector.pane?.kind !== pane) return;
    moveFocus ||= focus;
    dispatch?.(action, value);
  });
}

function button(label) {
  const node = document.createElement('button');
  node.type = 'button';
  node.textContent = label;
  return node;
}

function syncEditorAccessibility() {
  const editor = document.getElementById('gpui-text-input');
  const active = document.activeElement;
  const hasSearch = selector?.pane?.query != null;
  if (!hasSearch || active !== editor) {
    nativeSearchFocused = hasSearch && Boolean(controls?.pane.contains(active));
  }
  if (editor?.dataset.gpuiEditorActive === 'false' || nativeSearchFocused && active !== editor) editor?.setAttribute('aria-hidden', 'true');
  else editor?.removeAttribute('aria-hidden');
}

function createTeamDisableDialog() {
  const element = document.createElement('div');
  element.id = 'team-disable-accessibility';
  element.setAttribute('role', 'dialog');
  element.setAttribute('aria-modal', 'true');
  element.setAttribute('aria-labelledby', 'team-disable-accessibility-title');
  element.setAttribute('aria-describedby', 'team-disable-accessibility-description');
  element.tabIndex = -1;
  const title = document.createElement('h2');
  title.id = 'team-disable-accessibility-title';
  const close = button('关闭');
  const description = document.createElement('div');
  description.id = 'team-disable-accessibility-description';
  const error = document.createElement('div');
  error.setAttribute('role', 'alert');
  const status = document.createElement('div');
  status.setAttribute('role', 'status');
  const cancel = button('取消');
  const confirm = button('确认关闭');
  const buttons = [close, cancel, confirm];
  const act = action => {
    const request = teamDisable;
    if (!request || request.pending) return;
    queueMicrotask(() => {
      if (teamDisable?.session_id !== request.session_id) return;
      teamDialogFocus = true;
      dispatch?.(action, request.session_id);
    });
  };
  close.onclick = cancel.onclick = () => act('team-disable-cancel');
  confirm.onclick = () => act('team-disable-confirm');
  element.addEventListener('keydown', event => {
    if (event.isComposing || event.ctrlKey || event.altKey || event.metaKey) return;
    if (event.key === 'Escape' && !event.shiftKey) {
      if (!event.repeat) cancel.click();
    } else if (event.key === 'Tab') {
      const index = buttons.indexOf(document.activeElement);
      const target = teamDisable?.pending ? element : index === -1 ? cancel
        : buttons[(index + (event.shiftKey ? 2 : 1)) % buttons.length];
      target.focus({ preventScroll: true });
    } else return;
    event.preventDefault();
    event.stopPropagation();
  });
  element.append(title, close, description, error, status, cancel, confirm);
  return { element, title, description, error, status, cancel, confirm, buttons };
}

function reconcileTeamDisable(current, request, previous, focus) {
  current.element.hidden = !request;
  if (!request) return;
  const opened = request.session_id !== previous?.session_id;
  if (opened) {
    current.title.textContent = request.title;
    current.confirm.textContent = request.confirm_label;
    current.description.replaceChildren(...request.description.map(text => {
      const paragraph = document.createElement('p');
      paragraph.textContent = text;
      return paragraph;
    }));
  }
  const message = request.error ?? '';
  if (current.error.textContent !== message) current.error.textContent = message;
  current.status.textContent = request.pending ? '正在应用…' : '';
  if (focus && request.pending) current.element.focus({ preventScroll: true });
  for (const control of current.buttons) control.disabled = request.pending;
  if (focus && !request.pending) {
    if (opened) current.cancel.focus({ preventScroll: true });
    else if (previous?.pending) current.confirm.focus({ preventScroll: true });
  }
}

function createPermissionRiskDialog() {
  const element = document.createElement('div');
  element.id = 'permission-risk-accessibility';
  element.setAttribute('role', 'dialog');
  element.setAttribute('aria-modal', 'true');
  const title = document.createElement('h2');
  title.id = 'permission-risk-title';
  element.setAttribute('aria-labelledby', title.id);
  const description = document.createElement('p');
  description.id = 'permission-risk-description';
  element.setAttribute('aria-describedby', description.id);
  const close = button('关闭');
  const acknowledge = document.createElement('input');
  acknowledge.type = 'checkbox';
  const label = document.createElement('label');
  const acknowledgement = document.createElement('span');
  label.append(acknowledge, acknowledgement);
  const cancel = button('取消');
  const confirm = button('启用');
  const controls = [close, acknowledge, cancel, confirm];
  const act = action => {
    const request = permissionRisk;
    if (!request) return;
    queueMicrotask(() => {
      if (permissionRisk?.session !== request.session || permissionRisk?.preset !== request.preset) return;
      riskDialogFocus = true;
      dispatch?.(`permission-risk-${action}`, JSON.stringify([request.session, request.preset]));
    });
  };
  close.onclick = cancel.onclick = () => act('cancel');
  acknowledge.onchange = () => act('acknowledge');
  confirm.onclick = () => act('confirm');
  element.addEventListener('keydown', event => {
    if (event.isComposing || event.ctrlKey || event.altKey || event.metaKey) return;
    if (event.key === 'Escape' && !event.shiftKey) {
      if (!event.repeat) cancel.click();
    } else if (event.key === 'Tab') {
      const enabled = controls.filter(control => !control.disabled);
      const index = enabled.indexOf(document.activeElement);
      const target = index === -1 ? acknowledge
        : enabled[(index + (event.shiftKey ? enabled.length - 1 : 1)) % enabled.length];
      target.focus({ preventScroll: true });
    } else return;
    event.preventDefault();
    event.stopPropagation();
  });
  element.append(title, close, description, label, cancel, confirm);
  return { element, title, description, acknowledgement, acknowledge, confirm };
}

function reconcilePermissionRisk(current, request, previous) {
  current.element.hidden = !request;
  if (!request) return;
  const opened = request.session !== previous?.session || request.preset !== previous?.preset;
  if (opened) {
    current.title.textContent = request.title;
    current.description.textContent = request.description;
    current.acknowledgement.textContent = request.acknowledgement;
    current.confirm.textContent = request.action;
  }
  current.acknowledge.checked = request.acknowledged;
  current.confirm.disabled = !request.acknowledged;
  if (opened && riskDialogFocus) current.acknowledge.focus({ preventScroll: true });
}

function createSelector() {
  const root = document.createElement('div');
  root.id = 'model-accessibility';
  root.className = 'workspace-accessibility-surface';
  const commands = button('调用指令');
  commands.setAttribute('aria-haspopup', 'listbox');
  commands.setAttribute('aria-controls', 'model-accessibility-choices');
  commands.onclick = () => send('toggle-commands', '', true);
  const commandLauncher = document.createElement('div');
  commandLauncher.append(commands);
  const permission = button('访问模式');
  permission.setAttribute('aria-haspopup', 'menu');
  permission.setAttribute('aria-controls', 'model-accessibility-pane');
  permission.onclick = () => send('toggle-permission', '', true);
  const permissionLauncher = document.createElement('div');
  permissionLauncher.append(permission);
  const trigger = button('请选择模型');
  trigger.setAttribute('aria-haspopup', 'menu');
  trigger.setAttribute('aria-controls', 'model-accessibility-pane');
  trigger.onclick = () => send('toggle', '', true);
  const pane = document.createElement('div');
  pane.id = 'model-accessibility-pane';
  const search = document.createElement('input');
  search.type = 'text';
  search.setAttribute('role', 'searchbox');
  search.setAttribute('aria-label', '搜索模型…');
  search.setAttribute('aria-controls', 'model-accessibility-choices');
  search.addEventListener('compositionstart', () => { composing = true; });
  search.addEventListener('compositionend', () => {
    composing = false;
    send('query', search.value);
  });
  search.addEventListener('input', event => {
    if (!event.isComposing && !composing) send('query', search.value);
  });
  const clear = button('清除搜索');
  clear.onclick = () => {
    search.focus({ preventScroll: true });
    send('query');
  };
  const status = document.createElement('div');
  status.setAttribute('role', 'status');
  const errors = document.createElement('div');
  errors.setAttribute('aria-live', 'polite');
  const retry = button('重试加载模型');
  retry.onclick = () => send('retry', '', true);
  const choices = document.createElement('div');
  choices.id = 'model-accessibility-choices';
  pane.append(search, clear, status, errors, retry, choices);
  const dialog = createTeamDisableDialog();
  const risk = createPermissionRiskDialog();
  root.append(commandLauncher, permissionLauncher, trigger, pane, dialog.element, risk.element);
  root.addEventListener('keydown', navigate);
  root.addEventListener('focusin', syncEditorAccessibility);
  root.addEventListener('focusout', event => {
    queueMicrotask(syncEditorAccessibility);
    // GPUI can refocus its own textbox while rebinding the same search editor.
    if (event.relatedTarget?.id === 'gpui-text-input') return;
    if (selector?.pane && event.relatedTarget && !root.contains(event.relatedTarget)) send('dismiss');
  });
  window.addEventListener('pointerdown', event => {
    if (!root.contains(event.target)) {
      nativeSearchFocused = false;
      permissionReturnFocus = false;
    }
  }, true);
  document.body.append(root);
  return { root, commands, permission, trigger, pane, search, clear, status, errors, retry, choices, dialog, risk, rows: [] };
}

function navigate(event) {
  if (!selector || event.isComposing || composing || event.altKey || event.ctrlKey || event.metaKey) return;
  const { key, target, shiftKey } = event;
  const { pane } = selector;
  const arrow = key === 'ArrowDown' || key === 'ArrowUp';
  const back = key === 'Escape' || key === 'Tab' && shiftKey;
  if (!pane) {
    if ((target !== controls.trigger && target !== controls.permission) || !arrow || shiftKey) return;
    event.preventDefault();
    send(target === controls.permission ? 'toggle-permission' : 'toggle', '', true);
    return;
  }
  if (!back && (shiftKey || target === controls.retry || target === controls.clear)) return;
  if (key === 'Tab' && !back && controls.rows.includes(target) && !target.disabled) {
    event.preventDefault();
    if (!event.repeat) target.click();
    return;
  }
  if (target === controls.search && key === 'Tab' && !back && !controls.rows.length && !controls.retry.hidden) {
    event.preventDefault();
    controls.retry.focus({ preventScroll: true });
    return;
  }
  const submitSearch = target === controls.search && (key === 'Enter' || key === 'Tab')
    && (pane.busy || controls.rows.length > 0);
  const enterRoot = key === 'Tab' && (target === controls.trigger && pane.kind === 'root'
    || target === controls.permission && pane.kind === 'permission');
  const permissionEdge = pane.kind === 'permission' && (key === 'Home' || key === 'End');
  if (!back && !arrow && !submitSearch && !enterRoot && !permissionEdge) return;
  event.preventDefault();
  if (!event.repeat || arrow) send('key', shiftKey ? `shift+${key}` : key, true);
}

const paneLabels = {
  root: '模型与推理等级', model: '模型与推理等级', command: '/model 选项', commands: '调用指令',
  effort: '推理等级', team: '智能体团队协作',
  permission: '访问模式', 'permission-command': '/permission 选项',
};

function populateChoices(groups, kind, busy) {
  const fragment = document.createDocumentFragment();
  const rows = [];
  for (const group of groups) {
    const named = group.label && (groups.length > 1 || group.label !== '模型');
    const section = named ? document.createElement('div') : fragment;
    if (named) {
      section.setAttribute('role', 'group');
      section.setAttribute('aria-label', group.label);
      fragment.append(section);
    }
    for (const choice of group.choices) {
      const row = button(choice.label);
      row.id = `model-accessibility-choice-${rows.length}`;
      row.dataset.value = choice.value;
      if (kind === 'command' || kind === 'commands' || kind === 'permission-command') {
        row.setAttribute('role', 'option');
        if (choice.checked) row.textContent = `${choice.label}，${kind === 'permission-command' ? '当前模式' : '当前模型'}`;
      } else {
        row.setAttribute('role', kind === 'team' ? 'switch' : (choice.checked == null ? 'menuitem' : 'menuitemradio'));
        if (choice.checked != null) row.setAttribute('aria-checked', String(choice.checked));
      }
      row.disabled = busy || choice.disabled;
      row.onclick = () => send('choose', choice.value, true);
      rows.push(row);
      section.append(row);
    }
  }
  controls.choices.replaceChildren(fragment);
  controls.rows = rows;
}

function reconcile(next, nextDialog, nextRisk) {
  const previous = selector;
  const previousDialog = teamDisable;
  const previousRisk = permissionRisk;
  const previousSearchFocus = nativeSearchFocused;
  selector = next;
  teamDisable = nextDialog;
  permissionRisk = nextRisk;
  syncEditorAccessibility();
  if (!next && !nextDialog && !nextRisk && !controls) return;
  controls ??= createSelector();
  const { root, commands, permission, trigger, pane, search, clear, status, errors, retry, choices, dialog, risk } = controls;
  const focused = document.activeElement;
  const editor = document.getElementById('gpui-text-input');
  if (next?.input_label) editor?.setAttribute('aria-label', next.input_label);
  else editor?.removeAttribute('aria-label');
  if (next?.pane?.kind !== 'commands') {
    editor?.removeAttribute('aria-controls');
    editor?.removeAttribute('aria-activedescendant');
  }
  const returnFromRisk = previousRisk && riskDialogFocus;
  const requestedFocus = moveFocus || previousDialog && teamDialogFocus || returnFromRisk;
  moveFocus = false;
  root.hidden = !next && !nextDialog && !nextRisk;
  if (nextRisk) {
    riskDialogFocus ||= requestedFocus || root.contains(focused);
    reconcilePermissionRisk(risk, nextRisk, previousRisk);
    commands.hidden = permission.hidden = trigger.hidden = pane.hidden = true;
    dialog.element.hidden = true;
    composing = false;
    return;
  }
  risk.element.hidden = true;
  riskDialogFocus = false;
  if (nextDialog) {
    teamDialogFocus ||= requestedFocus || root.contains(focused);
    reconcileTeamDisable(dialog, nextDialog, previousDialog, teamDialogFocus);
    commands.hidden = true;
    permission.hidden = true;
    trigger.hidden = true;
    pane.hidden = true;
    composing = false;
    return;
  }
  dialog.element.hidden = true;
  teamDialogFocus = false;
  if (!next) {
    composing = false;
    commandReturnFocus = undefined;
    permissionReturnFocus = false;
    return;
  }
  const open = next.pane;
  const last = previous?.pane;
  const command = open?.kind === 'command' || open?.kind === 'permission-command';
  const permissionMenu = open?.kind === 'permission';
  const commandList = open?.kind === 'commands';
  const listbox = command || commandList;
  if (command && open.kind !== last?.kind) commandReturnFocus = editor ?? focused;
  commands.hidden = command;
  commands.setAttribute('aria-expanded', String(commandList));
  trigger.textContent = next.label ?? '';
  trigger.setAttribute('aria-expanded', String(Boolean(open && !listbox && !permissionMenu)));
  trigger.setAttribute('aria-busy', String(next.loading || next.busy));
  trigger.hidden = !next.label || command;
  permission.hidden = !next.permission_label || command;
  permission.textContent = next.permission_label ?? '';
  permission.setAttribute('aria-expanded', String(permissionMenu));
  permission.setAttribute('aria-busy', String(next.permission_busy));
  permission.disabled = next.permission_busy;
  pane.hidden = !open;
  if (!open) {
    composing = false;
    if (requestedFocus || previousSearchFocus || pane.contains(focused)) {
      const returnToEditor = last?.kind === 'command' || last?.kind === 'permission-command'
        || returnFromRisk && previousRisk.from_command;
      const target = returnToEditor ? commandReturnFocus ?? editor
        : last?.kind === 'commands' ? editor ?? commands
        : returnFromRisk || last?.kind === 'permission' ? permission : trigger;
      if (target === permission && permission.disabled) permissionReturnFocus = true;
      else target?.focus({ preventScroll: true });
    }
    if (permissionReturnFocus && !permission.disabled) {
      permissionReturnFocus = false;
      if (!permission.hidden && (document.activeElement === document.body || document.activeElement === editor)) {
        permission.focus({ preventScroll: true });
      }
    }
    commandReturnFocus = undefined;
    return;
  }
  permissionReturnFocus = false;

  const hasSearch = open.query != null;
  const changedPane = open.kind !== last?.kind;
  if (changedPane) composing = false;
  // Search and its live regions stay mounted while filtering or retrying.
  pane.setAttribute('role', 'group');
  pane.setAttribute('aria-label', paneLabels[open.kind]);
  choices.setAttribute('role', listbox ? 'listbox' : (open.kind === 'team' ? 'group' : 'menu'));
  if (command) choices.setAttribute('aria-label', open.kind === 'permission-command' ? '/permission 匹配项' : '/model 匹配项');
  else if (commandList) choices.setAttribute('aria-label', '触发候选建议');
  else if (open.kind === 'model') choices.setAttribute('aria-label', '模型');
  else choices.removeAttribute('aria-label');
  search.setAttribute('role', command ? 'textbox' : 'searchbox');
  search.setAttribute('aria-label', command ? '筛选选项' : '搜索模型…');
  search.placeholder = open.kind === 'permission-command' ? '搜索…' : '搜索模型…';
  search.hidden = !hasSearch;
  search.readOnly = open.busy;
  if (!composing && search.value !== (open.query ?? '')) search.value = open.query ?? '';
  clear.hidden = !hasSearch || !open.query;
  clear.disabled = open.busy;
  const message = open.status ?? '';
  if (status.textContent !== message) status.textContent = message;
  if (JSON.stringify(open.errors) !== JSON.stringify(last?.errors)) {
    errors.replaceChildren(...open.errors.map(message => {
      const line = document.createElement('div');
      line.textContent = message;
      return line;
    }));
  }
  retry.hidden = !open.retry;
  retry.textContent = open.kind === 'permission-command' ? '重试加载权限' : '重试加载模型';
  if (changedPane || open.busy !== last?.busy || JSON.stringify(open.groups) !== JSON.stringify(last.groups)) {
    populateChoices(open.groups, open.kind, open.busy);
  }
  const active = controls.rows[open.active];
  for (const row of controls.rows) {
    row.tabIndex = !hasSearch && row === (active ?? controls.rows[0]) ? 0 : -1;
    if (listbox) row.setAttribute('aria-selected', String(row === active));
  }
  if (commandList) {
    editor?.setAttribute('aria-controls', choices.id);
    if (active) {
      choices.setAttribute('aria-activedescendant', active.id);
      editor?.setAttribute('aria-activedescendant', active.id);
    } else {
      choices.removeAttribute('aria-activedescendant');
      editor?.removeAttribute('aria-activedescendant');
    }
    if (requestedFocus) (editor ?? active ?? commands).focus({ preventScroll: true });
    return;
  }
  choices.removeAttribute('aria-activedescendant');
  if (active && hasSearch) search.setAttribute('aria-activedescendant', active.id);
  else search.removeAttribute('aria-activedescendant');

  if (requestedFocus || changedPane && root.contains(focused) || hasSearch && nativeSearchFocused && focused === editor) {
    const target = (hasSearch ? search : null)
      ?? (active && !active.disabled ? active : null)
      ?? (!retry.hidden ? retry : permissionMenu ? permission : trigger);
    target.focus({ preventScroll: true });
  } else if (focused?.dataset.value && !focused.isConnected) {
    const replacement = controls.rows.find(row => row.dataset.value === focused.dataset.value);
    (replacement && !replacement.disabled ? replacement : hasSearch ? search : trigger)
      .focus({ preventScroll: true });
  } else if (focused === search && search.hidden || focused === retry && retry.hidden) {
    (hasSearch ? search : active ?? trigger).focus({ preventScroll: true });
  }
}

export function update_model_selector(next, team, risk) {
  // GPUI can update the textarea used for canvas input during the same draw.
  // Apply native focus after the draw and discard intermediate snapshots.
  if (!pending) queueMicrotask(() => {
    const model = pending;
    pending = undefined;
    reconcile(...model);
  });
  pending = [next, team, risk];
}
