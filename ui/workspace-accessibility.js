// The canvas owns the visible sidebar. These DOM items expose the same rows to
// the webview's accessibility API and forward activation to the canvas handlers.
let tree;
let search;
let dispatch;
let pendingRows;
let overlay;
let returnTo;
const items = new Map();

export function listen_workspace_tree(callback) {
  dispatch = callback;
}

function createTree() {
  const root = document.createElement('div');
  root.id = 'workspace-accessibility-tree';
  root.setAttribute('role', 'tree');
  root.setAttribute('aria-label', '工作区和会话');
  root.hidden = true;
  root.addEventListener('focusin', event => {
    const row = event.target.closest('[data-workspace-row]');
    if (row) {
      setTabStop(row);
      dispatch?.('focus', row.dataset.workspaceRow);
    }
  });
  root.addEventListener('focusout', () => queueMicrotask(() => {
    if (!root.contains(document.activeElement)) dispatch?.('focus', '');
  }));
  root.addEventListener('keydown', navigate);
  document.body.append(root);
  return root;
}

function setTabStop(target) {
  const previous = tree.querySelector('[data-workspace-row][tabindex="0"]');
  if (previous !== target) {
    if (previous) previous.tabIndex = -1;
    if (target) target.tabIndex = 0;
  }
}

function targetRowKey(target) {
  return target.kind === 'session' ? `session/${target.id}` : `group/${target.id ?? target.path}`;
}

function sameTarget(first, second) {
  return first?.kind === second.kind && first.id === second.id && first.path === second.path;
}

function navigate(event) {
  const row = event.target.closest('[data-workspace-row]');
  if (!row || event.altKey || event.ctrlKey || event.metaKey) return;
  if (event.key === 'ContextMenu' || event.key === 'F10' && event.shiftKey) {
    const menuButton = items.get(row.dataset.workspaceRow)?.menuButton;
    if (menuButton) {
      event.preventDefault();
      event.stopPropagation();
      if (!event.repeat) menuButton.click();
    }
    return;
  }
  if (event.shiftKey) return;
  const rows = [...tree.querySelectorAll('[data-workspace-row]')];
  let target;
  let activate = false;
  switch (event.key) {
    case 'ArrowDown': target = rows[rows.indexOf(row) + 1]; break;
    case 'ArrowUp': target = rows[rows.indexOf(row) - 1]; break;
    case 'Home': target = rows[0]; break;
    case 'End': target = rows.at(-1); break;
    case 'ArrowRight':
      activate = row.getAttribute('aria-expanded') === 'false';
      if (!activate) target = row.querySelector('[data-workspace-row]');
      break;
    case 'ArrowLeft':
      activate = row.getAttribute('aria-expanded') === 'true';
      if (!activate) target = row.parentElement.closest('[data-workspace-row]');
      break;
    case 'Enter':
    case ' ': activate = !event.repeat; break;
    default: return;
  }
  event.preventDefault();
  event.stopPropagation();
  if (activate) {
    if (event.target instanceof HTMLButtonElement) event.target.click();
    else row.click();
  }
  target?.focus({ preventScroll: true });
}

function createSearch() {
  const root = document.createElement('div');
  root.className = 'workspace-accessibility-surface';
  root.id = 'workspace-accessibility-search';
  const open = document.createElement('button');
  open.type = 'button';
  open.textContent = '搜索会话';
  const field = document.createElement('input');
  field.type = 'text';
  field.setAttribute('aria-label', '搜索会话名称');
  field.maxLength = 500;
  const clear = document.createElement('button');
  clear.type = 'button';
  clear.textContent = '清除搜索';
  const status = document.createElement('div');
  status.setAttribute('role', 'status');
  const controls = { root, open, field, clear, status, focusInput: false, restoreFocus: false, composing: false };
  const send = (action, value = '') => queueMicrotask(() => {
    if (!root.hidden) dispatch?.(action, value);
  });
  open.onclick = () => {
    if (!field.hidden) {
      field.focus({ preventScroll: true });
      return;
    }
    controls.focusInput = true;
    send('search-open');
  };
  const close = () => {
    controls.restoreFocus = true;
    controls.composing = false;
    send('search-close');
  };
  clear.onclick = close;
  field.addEventListener('compositionstart', () => { controls.composing = true; });
  field.addEventListener('compositionend', () => {
    controls.composing = false;
    if (!field.hidden) send('search-draft', field.value);
  });
  field.addEventListener('input', event => {
    if (!event.isComposing && !controls.composing) send('search-draft', field.value);
  });
  root.addEventListener('keydown', event => {
    if (event.key !== 'Escape' || event.isComposing || field.hidden) return;
    event.preventDefault();
    event.stopPropagation();
    close();
  });
  root.append(open, field, clear, status);
  tree.before(root);
  return controls;
}

function reconcileSearch(model) {
  search ??= createSearch();
  const { root, open, field, clear, status } = search;
  root.hidden = !model;
  if (!model) {
    search.focusInput = false;
    search.restoreFocus = false;
    search.composing = false;
    return;
  }
  const expanded = model.draft != null;
  open.setAttribute('aria-expanded', expanded);
  field.hidden = !expanded;
  clear.hidden = !expanded;
  if (!search.composing && field.value !== (model.draft ?? '')) field.value = model.draft ?? '';
  if (status.textContent !== (model.status ?? '')) status.textContent = model.status ?? '';
  if (expanded && search.focusInput) {
    search.focusInput = false;
    field.focus({ preventScroll: true });
  }
  if (!expanded && search.restoreFocus) {
    search.restoreFocus = false;
    open.focus({ preventScroll: true });
  }
}

function reconcile(rows, surface, searchModel) {
  tree ??= createTree();
  reconcileSearch(searchModel);
  tree.setAttribute('aria-label', searchModel?.results ? '搜索结果' : '工作区和会话');
  const active = tree.contains(document.activeElement) ? document.activeElement : null;
  const focusAncestors = [];
  for (let node = active; node && node !== tree; node = node.parentElement) {
    if (node.dataset.workspaceRow !== undefined) focusAncestors.push(node);
  }
  const present = new Set(rows.map(row => row.key));
  for (const [key, item] of items) {
    if (!present.has(key)) {
      item.element.remove();
      items.delete(key);
    }
  }

  const parents = [tree];
  const placed = new Map();
  let selected;
  for (const [index, row] of rows.entries()) {
    const parent = parents[row.depth];
    let item = items.get(row.key);
    if (!item) {
      const element = document.createElement('div');
      element.setAttribute('role', 'treeitem');
      element.dataset.workspaceRow = row.key;
      element.tabIndex = -1;
      const button = document.createElement('button');
      button.type = 'button';
      button.tabIndex = -1;
      button.onclick = event => {
        event.stopPropagation();
        dispatch?.('activate', row.key);
      };
      element.append(button);
      element.onclick = event => {
        if (event.target.closest('[data-workspace-row]') === element) dispatch?.('activate', row.key);
      };
      item = { element, button };
      items.set(row.key, item);
    }
    const { element, button } = item;
    for (const [attribute, value] of [
      ['aria-label', row.label], ['aria-description', row.description], ['aria-level', row.depth + 1],
      ['aria-expanded', row.expanded], ['aria-selected', row.selected],
    ]) {
      if (value == null) element.removeAttribute(attribute);
      else if (element.getAttribute(attribute) !== String(value)) element.setAttribute(attribute, value);
    }
    // A native button exposes AXPress even when WebKit treats activation of
    // the treeitem itself as selection or as a disclosure attribute write.
    const verb = row.expanded === true ? '折叠' : row.expanded === false ? '展开' : row.selected != null ? '打开' : '';
    const actionLabel = verb ? `${verb} ${row.label}` : row.label;
    if (button.textContent !== actionLabel) button.textContent = actionLabel;
    if (row.expanded != null) button.setAttribute('aria-expanded', row.expanded);
    if (row.menu) {
      if (!item.menuButton) {
        item.menuButton = document.createElement('button');
        item.menuButton.type = 'button';
        item.menuButton.tabIndex = -1;
        item.menuButton.setAttribute('aria-haspopup', 'menu');
        item.menuButton.onclick = event => {
          event.stopPropagation();
          returnTo = row.key;
          dispatch?.('open-menu', row.key);
        };
        element.append(item.menuButton);
      }
      const subject = row.key.startsWith('group/') ? '工作区' : '会话';
      const title = `${subject}“${row.label}”的操作`;
      if (item.menuButton.textContent !== title) item.menuButton.textContent = title;
      item.menuButton.setAttribute('aria-expanded', surface?.kind === 'menu' && row.key === targetRowKey(surface.target));
    } else if (item.menuButton) {
      item.menuButton.remove();
      item.menuButton = undefined;
    }
    if (row.unarchive) {
      if (!item.unarchiveButton) {
        item.unarchiveButton = document.createElement('button');
        item.unarchiveButton.type = 'button';
        item.unarchiveButton.tabIndex = 0;
        item.unarchiveButton.textContent = '取消归档';
        item.unarchiveButton.onclick = event => {
          event.stopPropagation();
          dispatch?.('unarchive', row.key);
        };
        element.append(item.unarchiveButton);
      }
    } else if (item.unarchiveButton) {
      item.unarchiveButton.remove();
      item.unarchiveButton = undefined;
    }
    if (searchModel?.results && row.description) {
      if (!item.details) {
        item.details = document.createElement('span');
        element.append(item.details);
      }
      if (item.details.textContent !== row.description) item.details.textContent = row.description;
    } else if (item.details) {
      item.details.remove();
      item.details = undefined;
    }
    if (row.selected) selected = element;
    const previous = placed.get(parent);
    const before = previous ? previous.nextElementSibling : parent.firstElementChild;
    if (element !== before) parent.insertBefore(element, before);
    placed.set(parent, element);

    if (rows[index + 1]?.depth > row.depth) {
      if (!item.group) {
        item.group = document.createElement('div');
        item.group.setAttribute('role', 'group');
        element.append(item.group);
      }
      parents[row.depth + 1] = item.group;
    } else if (item.group) {
      item.group.remove();
      item.group = undefined;
    }
  }
  tree.hidden = rows.length === 0 || Boolean(surface);
  const focused = focusAncestors.find(node => tree.contains(node));
  const fallback = focused ?? selected ?? tree.firstElementChild;
  setTabStop(fallback);
  const focusTarget = tree.contains(active) ? active : fallback;
  if (!tree.hidden && active && focusTarget && focusTarget !== document.activeElement) focusTarget.focus({ preventScroll: true });
  if (tree.hidden && active && searchModel?.draft != null) search.field.focus({ preventScroll: true });
  reconcileSurface(surface, active, fallback);
}

function sendSurface(current, action, fields = {}) {
  const target = current.model.target;
  // GPUI can change DOM focus during an action. Defer the resulting change
  // and focus events until the current App borrow has been released.
  queueMicrotask(() => {
    if (overlay !== current || !current.element.isConnected || !sameTarget(current.model.target, target)) return;
    dispatch?.('surface', JSON.stringify({ action, target, ...fields }));
  });
}

function createSurface(model) {
  const element = document.createElement('div');
  element.className = 'workspace-accessibility-surface';
  const current = { element, model };
  if (model.kind === 'menu') {
    element.setAttribute('role', 'menu');
    current.buttons = new Map();
    element.addEventListener('focusin', event => {
      const key = event.target.dataset.menuKey;
      if (key) sendSurface(current, 'menu-focus', { key });
    });
    element.addEventListener('focusout', () => queueMicrotask(() => {
      if (!element.contains(document.activeElement)) sendSurface(current, 'menu-focus', { key: '' });
    }));
    element.addEventListener('keydown', event => {
      if (event.altKey || event.ctrlKey || event.metaKey) return;
      const available = [...element.querySelectorAll('button:not(:disabled)')];
      const index = available.indexOf(document.activeElement);
      let target;
      switch (event.key) {
        case 'ArrowDown': target = available[(index + 1) % available.length]; break;
        case 'ArrowUp': target = available[(index < 0 ? available.length - 1 : (index + available.length - 1) % available.length)]; break;
        case 'Home': target = available[0]; break;
        case 'End': target = available.at(-1); break;
        case 'ArrowRight':
          if (document.activeElement?.getAttribute('aria-haspopup') !== 'menu') return;
          if (!event.repeat) document.activeElement.click();
          break;
        case 'ArrowLeft':
          if (!current.model.submenu) return;
          sendSurface(current, 'menu-dismiss');
          break;
        case 'Escape':
        case 'Tab': sendSurface(current, 'menu-dismiss'); break;
        case 'Enter':
        case ' ': if (!event.repeat) return; break;
        default: return;
      }
      event.preventDefault();
      event.stopPropagation();
      target?.focus({ preventScroll: true });
    });
  } else {
    const rename = model.kind === 'rename';
    const workspace = model.target.kind === 'workspace';
    element.setAttribute('role', model.kind === 'archive' ? 'alertdialog' : 'dialog');
    element.setAttribute('aria-modal', 'true');
    element.setAttribute('aria-label', rename ? (workspace ? '重命名工作区' : '重命名会话')
      : (workspace ? '移除工作区' : '停止并归档此会话？'));
    if (rename) {
      const field = document.createElement('input');
      field.type = 'text';
      field.setAttribute('aria-label', workspace ? '工作区名称' : '会话名称');
      field.autocomplete = 'off';
      const publishDraft = () => {
        current.confirm.disabled = current.model.pending || !field.value.trim() || workspace && current.model.draft !== field.value;
        sendSurface(current, 'rename-draft', { value: field.value });
      };
      field.addEventListener('input', event => { if (!event.isComposing && !current.composing) publishDraft(); });
      field.addEventListener('change', () => { if (!current.composing) publishDraft(); });
      field.addEventListener('compositionstart', () => { current.composing = true; });
      field.addEventListener('compositionend', () => {
        current.composing = false;
        publishDraft();
      });
      current.field = field;
      element.append(field);
    } else {
      const description = document.createElement('p');
      description.id = 'workspace-action-description';
      element.setAttribute('aria-describedby', description.id);
      description.textContent = model.description;
      element.append(description);
      if (model.activity?.length) {
        const activity = document.createElement('ul');
        for (const line of model.activity) {
          const entry = document.createElement('li');
          entry.textContent = line;
          activity.append(entry);
        }
        element.append(activity);
      }
    }
    current.error = document.createElement('div');
    current.error.setAttribute('role', 'alert');
    current.status = document.createElement('div');
    current.status.setAttribute('role', 'status');
    current.cancel = document.createElement('button');
    current.cancel.type = 'button';
    current.cancel.textContent = '取消';
    current.cancel.onclick = () => sendSurface(current, `${model.kind}-cancel`);
    current.confirm = document.createElement('button');
    current.confirm.type = 'button';
    current.confirm.textContent = rename ? (workspace ? '重命名' : '保存')
      : (workspace ? '移除工作区' : '停止并归档');
    current.confirm.onclick = () => {
      if (!current.composing) sendSurface(current, `${model.kind}-confirm`, rename ? { value: current.field.value } : {});
    };
    element.append(current.error, current.status, current.cancel, current.confirm);
    element.addEventListener('keydown', event => {
      if (current.composing || event.isComposing || event.keyCode === 229) return;
      if (event.key === 'Escape') current.cancel.click();
      else if (event.key === 'Enter' && event.target === current.field) {
        if (!event.repeat && !current.model.pending) {
          sendSurface(current, 'rename-confirm', { value: current.field.value });
        }
      } else if (event.key === 'Tab') {
        const controls = [...element.querySelectorAll('input:not(:disabled), button:not(:disabled)')];
        const index = controls.indexOf(document.activeElement);
        controls[(index + controls.length + (event.shiftKey ? -1 : 1)) % controls.length]?.focus({ preventScroll: true });
      } else return;
      event.preventDefault();
      event.stopPropagation();
    });
  }
  document.body.append(element);
  return current;
}

function updateSurface(current, model) {
  current.model = model;
  if (model.kind === 'menu') {
    current.element.setAttribute('aria-label', model.label);
    const live = new Set(model.items.map(item => item.key));
    for (const [key, button] of current.buttons) {
      if (!live.has(key)) {
        button.remove();
        current.buttons.delete(key);
      }
    }
    let before = current.element.firstElementChild;
    for (const item of model.items) {
      let button = current.buttons.get(item.key);
      if (!button) {
        button = document.createElement('button');
        button.type = 'button';
        button.setAttribute('role', 'menuitem');
        button.dataset.menuKey = item.key;
        button.onclick = () => sendSurface(current, 'menu-item', { key: item.key });
        current.buttons.set(item.key, button);
      }
      if (button.textContent !== item.label) button.textContent = item.label;
      button.disabled = item.disabled;
      if (item.submenu) button.setAttribute('aria-haspopup', 'menu');
      if (button !== before) current.element.insertBefore(button, before);
      before = button.nextElementSibling;
    }
  } else {
    if (current.field) {
      if (!current.composing && current.field.value !== model.draft) current.field.value = model.draft;
      current.field.readOnly = model.pending;
    }
    current.cancel.disabled = model.pending;
    current.confirm.disabled = model.pending || Boolean(model.blocked);
    current.error.textContent = model.error ?? '';
    current.status.textContent = model.pending ? (current.field ? '正在保存…'
      : model.kind === 'remove-workspace' ? '正在移除工作区…' : '正在停止并归档…') : '';
  }
}

function reconcileSurface(model, activeTree, fallback) {
  const previous = overlay?.model;
  const inOverlay = overlay?.element.contains(document.activeElement);
  const initialFocus = Boolean(returnTo || activeTree || inOverlay);
  if (!model) {
    overlay?.element.remove();
    overlay = undefined;
    if (returnTo && !tree.hidden) {
      const item = items.get(returnTo);
      (item?.menuButton ?? item?.element ?? fallback)?.focus({ preventScroll: true });
      returnTo = undefined;
    }
    return;
  }
  if (activeTree && !returnTo) returnTo = activeTree.closest('[data-workspace-row]')?.dataset.workspaceRow;
  const changed = !previous || previous.kind !== model.kind || !sameTarget(previous.target, model.target);
  if (changed) {
    overlay?.element.remove();
    overlay = createSurface(model);
  }
  updateSurface(overlay, model);
  const pageChanged = previous?.submenu !== model.submenu;
  if (initialFocus && (changed || pageChanged || inOverlay && !overlay.element.contains(document.activeElement) || previous?.pending && !model.pending)) {
    const target = pageChanged && previous?.submenu && !model.submenu ? overlay.buttons?.get('open-in')
      : overlay.field ?? overlay.cancel ?? overlay.element.querySelector('button:not(:disabled)');
    target?.focus({ preventScroll: true });
    if (changed && overlay.field) overlay.field.select();
  }
}

export function update_workspace_tree(rows, surface, search) {
  // Returning focus to a visible ancestor can invoke a Rust listener. Do it
  // after GPUI has released its render borrow, and retain only the latest frame.
  if (!pendingRows) queueMicrotask(() => {
    const next = pendingRows;
    pendingRows = undefined;
    reconcile(...next);
  });
  pendingRows = [rows, surface, search];
}
