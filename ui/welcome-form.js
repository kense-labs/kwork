// The welcome credential belongs to a native password field until submission.
let dispatch;
let controls;
let state;

export function listen_welcome_form(callback) {
  dispatch = callback;
}

export function welcome_has_draft() {
  return (controls?.key.value.length ?? 0) > 0;
}

function setRevealed(revealed) {
  const { key, reveal } = controls;
  controls.revealedDraft = revealed ? key.value : undefined;
  key.type = revealed ? 'text' : 'password';
  reveal.textContent = revealed ? '隐藏' : '显示';
  reveal.setAttribute('aria-label', revealed ? '隐藏密钥' : '显示密钥');
  reveal.setAttribute('aria-pressed', String(revealed));
}

function syncActions() {
  const { key, reveal, save, providers, account, later } = controls;
  if (key.type === 'text' && key.value !== controls.revealedDraft) setRevealed(false);
  key.readOnly = state.saving;
  reveal.hidden = key.value.length === 0 || state.saving;
  save.disabled = state.checking || state.saving || key.value.trim() === '';
  const saveLabel = state.saving ? '保存中…' : '保存并继续';
  if (save.textContent !== saveLabel) save.textContent = saveLabel;
  providers.disabled = state.saving;
  account.disabled = state.saving;
  later.disabled = state.saving;
}

function mount() {
  const form = document.createElement('form');
  form.id = 'welcome-setup';
  form.setAttribute('aria-labelledby', 'welcome-setup-title');
  form.setAttribute('aria-describedby', 'welcome-setup-description');
  form.innerHTML = `
    <p class="welcome-loading" role="status">正在读取模型配置…</p>
    <div class="welcome-fields">
      <div>
        <h1 id="welcome-setup-title">添加一个 API Key 开始使用</h1>
        <p id="welcome-setup-description">配置 DeepSeek 官方模型，或添加自定义模型服务商。</p>
      </div>
      <div class="welcome-key-row">
        <input type="password" aria-label="API 密钥" placeholder="输入 API 密钥"
          autocomplete="new-password" autocapitalize="none" spellcheck="false"
          aria-describedby="welcome-setup-error">
        <button type="button" data-action="reveal" hidden></button>
      </div>
      <p id="welcome-setup-error" role="alert" hidden></p>
    </div>
    <div class="welcome-actions">
      <button type="submit" disabled>保存并继续</button>
      <button type="button" data-action="account" hidden>登录 DeepSeek 账号</button>
      <button type="button" data-action="providers">配置模型服务</button>
      <button type="button" data-action="later">稍后配置</button>
    </div>`;
  const key = form.querySelector('input');
  const reveal = form.querySelector('[data-action=reveal]');
  const save = form.querySelector('[type=submit]');
  const providers = form.querySelector('[data-action=providers]');
  const account = form.querySelector('[data-action=account]');
  const later = form.querySelector('[data-action=later]');
  const error = form.querySelector('[role=alert]');
  const fields = form.querySelector('.welcome-fields');
  const actions = form.querySelector('.welcome-actions');
  const loading = form.querySelector('[role=status]');
  controls = { form, key, reveal, save, providers, account, later, error, fields, actions, loading };

  form.addEventListener('submit', event => {
    event.preventDefault();
    if (!save.disabled) dispatch?.('welcome-save', key.value);
  });
  key.addEventListener('input', () => {
    if (key.type !== 'password') setRevealed(false);
    if (state.error) dispatch?.('welcome-edit', '');
    syncActions();
  });
  reveal.addEventListener('click', () => setRevealed(key.type === 'password'));
  providers.addEventListener('click', () => dispatch?.('welcome-providers', ''));
  account.addEventListener('click', () => dispatch?.('welcome-account', ''));
  later.addEventListener('click', () => dispatch?.('welcome-later', ''));
  form.addEventListener('keydown', event => {
    if (event.key === 'Escape' || (event.key === 'Enter' && (event.isComposing || event.repeat))) {
      event.preventDefault();
      return;
    }
    if (event.key !== 'Tab') return;
    const tabbable = [key, reveal, save, account, providers, later]
      .filter(item => !item.hidden && !item.disabled);
    const first = tabbable[0];
    const last = tabbable.at(-1);
    const active = document.activeElement;
    if (event.shiftKey ? active === first : active === last) {
      event.preventDefault();
      (event.shiftKey ? last : first).focus();
    }
  });
  setRevealed(false);
  document.body.append(form);
  document.documentElement.dataset.welcomeSetup = '';
  // WebKit's accessibility value setter can edit a password field without
  // dispatching an input event. Check the actual field while this form exists.
  controls.timer = setInterval(() => {
    if (state && !state.checking) syncActions();
  }, 150);
}

export function update_welcome_form(next) {
  const previous = state;
  state = next;
  if (!next) {
    if (controls) {
      clearInterval(controls.timer);
      controls.key.value = '';
      controls.form.remove();
      controls = undefined;
      delete document.documentElement.dataset.welcomeSetup;
    }
    return;
  }
  if (!controls) mount();
  const { form, key, error, fields, actions, loading } = controls;
  form.dataset.appearance = next.dark ? 'dark' : 'light';
  form.inert = next.account_open;
  controls.account.hidden = !next.account_available;
  if (previous?.account_open && !next.account_open && !next.checking) controls.account.focus();
  form.setAttribute('aria-busy', String(next.saving));
  fields.hidden = actions.hidden = next.checking;
  loading.hidden = !next.checking;
  if (next.checking) {
    form.removeAttribute('role');
    form.removeAttribute('aria-modal');
  } else {
    form.setAttribute('role', 'dialog');
    form.setAttribute('aria-modal', 'true');
    if (!previous || previous.checking) requestAnimationFrame(() => {
      if (controls?.form === form && !state?.checking && !form.contains(document.activeElement)) key.focus({ preventScroll: true });
    });
  }
  if (next.saving && !previous?.saving) {
    setRevealed(false);
    key.focus({ preventScroll: true });
  }
  error.textContent = next.error ?? '';
  error.hidden = !next.error;
  syncActions();
}
