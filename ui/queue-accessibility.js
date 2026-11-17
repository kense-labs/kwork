// System accessibility for the painted queue controls and the shared inline editor.
let dispatch;
let state;
let controls;
let focusEditor;

export function listen_inbox(callback) {
  dispatch = callback;
}

function send(action, item, text) {
  const session = state?.session;
  queueMicrotask(() => {
    if (session && session === state?.session) dispatch?.(action, JSON.stringify({ session, item, text }));
  });
}

function button(label, click) {
  const element = document.createElement('button');
  element.type = 'button';
  element.textContent = label;
  element.onclick = click;
  return element;
}

function createQueue() {
  const root = document.createElement('section');
  root.id = 'queue-accessibility';
  root.className = 'workspace-accessibility-surface';
  root.setAttribute('role', 'group');
  root.setAttribute('aria-label', '排队消息');
  const summary = button('', () => send('toggle'));
  summary.setAttribute('aria-controls', 'accessible-queue-list');
  const list = document.createElement('ul');
  list.id = 'accessible-queue-list';
  const status = document.createElement('div');
  status.setAttribute('role', 'status');
  const alert = document.createElement('div');
  alert.setAttribute('role', 'alert');
  root.append(summary, list, status, alert);
  document.body.append(root);
  return { root, summary, list, status, alert, rows: new Map(), pending: document.createElement('li') };
}

function updateAttachments(parent, labels) {
  const key = JSON.stringify(labels);
  if (parent.dataset.attachments === key) return;
  parent.dataset.attachments = key;
  parent.replaceChildren(...labels.map(label => {
    const item = document.createElement('span');
    if (label === '排队消息图片') {
      item.setAttribute('role', 'img');
      item.setAttribute('aria-label', label);
      item.className = 'accessible-queue-image';
    } else {
      item.textContent = label;
    }
    return item;
  }));
}

function createRow(id) {
  const row = document.createElement('li');
  const attachments = document.createElement('span');
  const preview = document.createElement('span');
  const actions = document.createElement('div');
  const edit = button('编辑排队消息', () => {
    focusEditor = id;
    send('edit', id);
  });
  const remove = button('删除排队消息', () => send('remove', id));
  const steer = button('插话发送', () => send('steer', id));
  actions.append(edit, remove, steer);
  row.append(attachments, preview, actions);
  return { row, preview, attachments, actions, edit, remove, steer, editor: null };
}

function createEditor(id, draft) {
  const group = document.createElement('div');
  const input = document.createElement('textarea');
  input.setAttribute('aria-label', '编辑排队消息');
  input.value = draft;
  const saveDraft = () => {
    if (!state?.busy && input.value.trim()) {
      input.focus({ preventScroll: true });
      send('save', id, input.value);
    }
  };
  const save = button('保存排队消息', saveDraft);
  const cancel = button('取消编辑', () => send('cancel', id));
  let composing = false;
  input.addEventListener('compositionstart', () => { composing = true; });
  input.addEventListener('compositionend', () => {
    composing = false;
    send('draft', id, input.value);
  });
  input.addEventListener('input', event => {
    if (!composing && !event.isComposing) send('draft', id, input.value);
  });
  input.addEventListener('keydown', event => {
    if (event.isComposing || event.keyCode === 229 || composing || state?.busy) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      send('cancel', id);
    } else if (event.key === 'Enter' && !event.shiftKey && !event.repeat) {
      event.preventDefault();
      saveDraft();
    }
  });
  group.append(input, save, cancel);
  return { group, input, save, cancel };
}

export function update_inbox(next) {
  const sessionChanged = state?.session !== next?.session;
  state = next;
  if (!next && !controls) return;
  controls ??= createQueue();
  const { root, summary, list, status, alert, rows, pending } = controls;
  const ownedFocus = root.contains(document.activeElement);
  root.hidden = !next;
  if (sessionChanged) {
    list.replaceChildren();
    rows.clear();
    focusEditor = undefined;
  }
  if (!next) {
    const composer = document.getElementById('gpui-text-input');
    if (ownedFocus && composer?.hasAttribute('aria-label')) composer.focus({ preventScroll: true });
    return;
  }
  const count = next.rows.length + Number(next.pending != null);
  summary.hidden = count < 2;
  summary.disabled = next.locked;
  summary.textContent = `${count} 条排队消息`;
  summary.setAttribute('aria-expanded', String(next.expanded));
  list.hidden = count > 1 && !next.expanded;
  status.textContent = next.pending != null ? '发送中…' : '';
  alert.hidden = !next.error;
  alert.textContent = next.error ?? '';

  const visibleIds = new Set(next.rows.map(row => row.id));
  for (const [id, row] of rows) {
    if (visibleIds.has(id)) continue;
    const wasFocused = row.row.contains(document.activeElement);
    row.row.remove();
    rows.delete(id);
    if (wasFocused && !summary.hidden && !summary.disabled) summary.focus({ preventScroll: true });
  }
  let position = list.firstChild;
  for (const data of next.rows) {
    let row = rows.get(data.id);
    if (!row) {
      row = createRow(data.id);
      rows.set(data.id, row);
    }
    const editing = data.id === next.editor?.id;
    row.preview.textContent = data.display;
    updateAttachments(row.attachments, data.attachments);
    row.preview.hidden = row.actions.hidden = row.attachments.hidden = editing;
    row.edit.disabled = next.busy || !data.editable;
    row.edit.title = data.editable ? '' : '包含非文本内容，暂不支持编辑';
    row.remove.disabled = next.busy;
    row.steer.disabled = next.busy || !next.running;
    row.steer.title = next.running ? '' : '仅运行中可插话发送';
    if (row.row !== position) list.insertBefore(row.row, position);
    position = row.row.nextSibling;
    if (editing) {
      if (!row.editor) {
        row.editor = createEditor(data.id, next.editor.draft);
        row.row.append(row.editor.group);
      }
      const { input, save, cancel } = row.editor;
      if (input !== document.activeElement && input.value !== next.editor.draft) input.value = next.editor.draft;
      input.readOnly = next.busy;
      save.disabled = next.busy || !next.editor.draft.trim();
      cancel.disabled = next.busy;
      if (focusEditor === data.id) {
        input.focus({ preventScroll: true });
        focusEditor = undefined;
      }
    } else if (row.editor) {
      const wasFocused = row.editor.group.contains(document.activeElement);
      row.editor.group.remove();
      row.editor = null;
      if (wasFocused && !row.edit.disabled) row.edit.focus({ preventScroll: true });
    }
  }
  if (next.pending != null) {
    if (!pending.firstChild) pending.append(document.createElement('span'), document.createElement('span'));
    updateAttachments(pending.firstChild, next.pending.attachments);
    pending.lastChild.textContent = next.pending.display;
    if (pending.parentNode !== list) list.append(pending);
  } else {
    pending.remove();
  }
}
