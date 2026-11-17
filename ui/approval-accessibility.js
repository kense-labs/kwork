// Native accessibility for the GPUI approval card. Both surfaces use the same request.
let controls;
let state;
let dispatch;
let composing = false;
let compositionEnded = false;

export function listen_approval(callback) {
  dispatch = callback;
}

function choose(id, option) {
  if (!option || state?.id !== id || state.answering) return;
  queueMicrotask(() => {
    if (state?.id === id && !state.answering) dispatch?.(id, option);
  });
}

function create() {
  const root = document.createElement('section');
  root.id = 'approval-accessibility';
  root.className = 'workspace-accessibility-surface';
  root.setAttribute('role', 'group');
  root.setAttribute('aria-label', '工具审批');

  const status = document.createElement('div');
  status.setAttribute('role', 'status');
  const details = document.createElement('div');
  details.setAttribute('role', 'group');
  details.setAttribute('aria-label', '审批详情');
  details.tabIndex = 0;
  const subject = document.createElement('div');
  const command = document.createElement('pre');
  const error = document.createElement('div');
  error.setAttribute('role', 'alert');
  const actions = document.createElement('div');
  details.append(subject, command);
  root.append(status, details, error, actions);
  document.body.append(root);

  root.addEventListener('compositionstart', () => { composing = true; });
  root.addEventListener('compositionend', () => {
    composing = false;
    compositionEnded = true;
  });
  root.addEventListener('keyup', () => { compositionEnded = false; });
  root.addEventListener('focusout', event => {
    if (!root.contains(event.relatedTarget)) composing = compositionEnded = false;
  });
  root.addEventListener('keydown', event => {
    if (!state || event.defaultPrevented || !root.contains(document.activeElement)) return;
    const enter = event.key === 'Enter';
    const escape = event.key === 'Escape';
    if (!enter && !escape && event.key !== ' ') return;
    if (event.repeat || event.isComposing || event.keyCode === 229 || composing || compositionEnded
        || event.metaKey || event.altKey || event.ctrlKey || event.shiftKey || state.answering) {
      event.preventDefault();
      return;
    }
    const option = escape ? state.reject : enter && event.target === details ? state.allow : null;
    if (option) {
      event.preventDefault();
      choose(state.id, option);
    }
  });
  return { root, status, details, subject, command, error, actions };
}

export function update_approval(next) {
  const previous = state;
  state = next;
  if (!controls && !next) return;
  controls ??= create();
  const { root, status, details, subject, command, error, actions } = controls;
  const focused = document.activeElement;
  const ownedFocus = root.contains(focused);
  root.hidden = !next;
  if (!next) {
    composing = compositionEnded = false;
    const editor = document.getElementById('gpui-text-input');
    if (ownedFocus && editor?.hasAttribute('aria-label')) editor.focus({ preventScroll: true });
    return;
  }

  const newRequest = previous?.id !== next.id;
  if (ownedFocus && (newRequest || (next.answering && actions.contains(focused)))) {
    details.focus({ preventScroll: true });
  }
  root.dataset.approvalKey = next.id;
  root.setAttribute('aria-busy', String(next.answering));
  status.textContent = next.answering ? '正在提交审批…' : '等待审批';
  subject.textContent = next.subject;
  command.hidden = !next.command;
  command.textContent = next.command ?? '';
  error.hidden = !next.error;
  error.textContent = next.error ?? '';

  if (newRequest) {
    const buttons = next.choices.map(({ id, label }) => {
      const button = document.createElement('button');
      button.type = 'button';
      button.textContent = label;
      button.addEventListener('click', () => choose(next.id, id));
      return button;
    });
    actions.replaceChildren(...buttons);
  }
  for (const button of actions.children) button.disabled = next.answering;
}
