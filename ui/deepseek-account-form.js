import { place_account_notices, listen_account_notice_open } from '/account-notifications.js';
import { listen_account_quota_notice } from '/account-quota-notice.js';
export { place_account_notice_cover, update_account_notices } from '/account-notifications.js';
export { update_account_quota_notice } from '/account-quota-notice.js';

let dispatch;
let controls;
let state;
let bounds;
let launcher;
let launcherState;
let launcherBounds;
let restoreLauncherFocus = false;
let avatar;
let avatarRetry;
let copyTimer;

export function account_authorization_url(url, dark) {
  const link = new URL(url);
  link.searchParams.set('theme', dark ? 'dark' : 'light');
  return link.href;
}

export function account_contact_url(id, version) {
  const link = new URL('https://trtgsjkv6r.feishu.cn/share/base/form/shrcnlCoGElW7MQznGy9r3YYXcg');
  const scale = devicePixelRatio || 1;
  const resolution = `${Math.round(screen.width * scale)}x${Math.round(screen.height * scale)}`;
  const fields = { uid: id, source: '', harness_version: version,
    device_info: `${navigator.userAgent}; screen_resolution=${resolution}` };
  for (const [key, value] of Object.entries(fields)) {
    link.searchParams.set(`hide_${key}`, '1');
    if (value) link.searchParams.set(`prefill_${key}`, value);
  }
  return link.href;
}

function paintAvatar(holder) {
  if (!holder) return;
  holder.dataset.avatarState = avatar?.status ?? 'empty';
  holder.replaceChildren();
  if (avatar?.status !== 'ready') return;
  const current = avatar;
  const image = document.createElement('img');
  image.alt = '';
  image.referrerPolicy = 'no-referrer';
  image.onerror = () => {
    if (avatar !== current || current.status !== 'ready') return;
    current.status = 'failed';
    paintAvatars();
  };
  image.src = current.url;
  holder.append(image);
}

function paintAvatars() {
  paintAvatar(launcher?.avatar);
  paintAvatar(controls?.parts.avatar);
}

export function update_account_avatar(raw, retry) {
  let url;
  try {
    const link = new URL(raw);
    if (link.protocol === 'https:' && !link.username && !link.password) url = link.href;
  } catch {}
  const retryFailed = avatar?.status === 'failed' && avatarRetry !== retry;
  avatarRetry = retry;
  if (avatar?.url === url && !retryFailed) return;
  if (avatar?.image) {
    avatar.image.onload = avatar.image.onerror = null;
    avatar.image.removeAttribute('src');
  }
  avatar = undefined;
  paintAvatars();
  if (!url) return;
  const image = new Image();
  const next = avatar = { url, image, status: 'loading' };
  paintAvatars();
  image.referrerPolicy = 'no-referrer';
  const finish = status => {
    if (avatar !== next) return;
    next.status = status;
    paintAvatars();
  };
  image.onload = () => finish('ready');
  image.onerror = () => finish('failed');
  image.src = url;
}

export function place_account_launcher(left, top, width, height) {
  launcherBounds = { left, top, width, height };
  place_account_notices(left, top, width);
  if (launcher) Object.assign(launcher.root.style, Object.fromEntries(
    Object.entries(launcherBounds).map(([key, value]) => [key, `${value}px`]),
  ));
}

export function focus_account_launcher() {
  if (launcher?.root.hidden || !launcherState) return;
  launcher.trigger.focus({ preventScroll: true });
  if (!launcherState.open) dispatch?.('account-menu-toggle', '');
}

function mountLauncher() {
  const root = document.createElement('div');
  root.id = 'deepseek-account-launcher';
  root.hidden = true;
  root.innerHTML = `
    <button type="button" aria-haspopup="menu" aria-controls="deepseek-launcher-menu">
      <span data-part="more" aria-hidden="true">⋯</span>
      <span class="account-avatar" data-part="avatar" aria-hidden="true" hidden></span>
      <span data-part="label">更多</span>
    </button>
    <div id="deepseek-launcher-menu" role="menu" aria-label="账号菜单" hidden>
      <button type="button" role="menuitem" data-action="settings" tabindex="-1">设置</button>
      <button type="button" role="menuitem" data-action="contact" tabindex="-1">联系我们</button>
      <button type="button" role="menuitem" data-action="sign-in" tabindex="-1">登录 DeepSeek 账号</button>
      <button type="button" role="menuitem" data-action="signout" tabindex="-1">退出登录</button>
    </div>`;
  const trigger = root.querySelector('button');
  const menu = root.querySelector('[role="menu"]');
  const buttons = [...menu.querySelectorAll('button')];
  const dismiss = restore => {
    restoreLauncherFocus = restore;
    dispatch?.('account-menu-dismiss', '');
  };
  trigger.onclick = () => dispatch?.('account-menu-toggle', '');
  buttons.forEach(button => { button.onclick = () => {
    restoreLauncherFocus = button.dataset.action !== 'contact';
    dispatch?.(`account-menu-${button.dataset.action}`, '');
  }; });
  root.addEventListener('keydown', event => {
    if (event.isComposing || !launcherState) return;
    const next = buttons.filter(button => !button.hidden);
    if (!launcherState.open && ['ArrowDown', 'ArrowUp'].includes(event.key)) {
      event.preventDefault();
      dispatch?.('account-menu-toggle', '');
    } else if (launcherState.open && event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      dismiss(true);
    } else if (launcherState.open && ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      const index = next.indexOf(document.activeElement);
      const target = event.key === 'Home' ? 0 : event.key === 'End' ? next.length - 1
        : (index + (event.key === 'ArrowDown' ? 1 : next.length - 1)) % next.length;
      next[target]?.focus({ preventScroll: true });
    }
  });
  root.addEventListener('focusout', event => {
    if (launcherState?.open && event.relatedTarget && !root.contains(event.relatedTarget)) dismiss(false);
  });
  document.addEventListener('pointerdown', event => {
    if (launcherState?.open && !root.contains(event.target)) dismiss(false);
  }, true);
  launcher = { root, trigger, menu, buttons,
    ...Object.fromEntries([...root.querySelectorAll('[data-part]')].map(node => [node.dataset.part, node])) };
  if (launcherBounds) place_account_launcher(...Object.values(launcherBounds));
  document.body.append(root);
  paintAvatar(launcher.avatar);
}

export function update_account_launcher(next) {
  const previous = launcherState;
  launcherState = next;
  if (!next) {
    if (launcher) launcher.root.hidden = true;
    return;
  }
  if (!launcher) mountLauncher();
  const { root, trigger, menu, buttons, label, more, avatar: holder } = launcher;
  root.hidden = false;
  root.dataset.appearance = next.dark ? 'dark' : 'light';
  root.dataset.compact = String(next.compact);
  trigger.setAttribute('aria-label', next.signedIn ? (next.label ? `账号菜单，${next.label}` : '账号菜单') : '更多设置');
  trigger.setAttribute('aria-expanded', String(next.open));
  label.textContent = next.signedIn ? next.label ?? '' : '更多';
  label.hidden = next.compact;
  more.hidden = next.signedIn;
  holder.hidden = !next.signedIn;
  buttons[2].hidden = next.signedIn;
  buttons[3].hidden = !next.signedIn;
  menu.hidden = !next.open;
  if (next.open && !previous?.open) buttons[0].focus({ preventScroll: true });
  else if (!next.open && restoreLauncherFocus) {
    restoreLauncherFocus = false;
    trigger.focus({ preventScroll: true });
  }
}

export function listen_deepseek_account_form(callback) {
  dispatch = callback;
  listen_account_notice_open(callback);
  listen_account_quota_notice(callback);
}

export function place_deepseek_account_form(left, top, width, height) {
  bounds = { left, top, width, height };
  place();
}

function place() {
  if (!controls || !state) return;
  for (const property of ['left', 'top', 'width', 'height']) {
    controls.root.style[property] = !state.onboarding && !state.launcherDialog && bounds ? `${bounds[property]}px` : '';
  }
}

function mount() {
  const root = document.createElement('section');
  root.id = 'deepseek-account-form';
  root.setAttribute('aria-labelledby', 'deepseek-account-title');
  root.innerHTML = `
    <div class="account-content">
      <header><h2 id="deepseek-account-title" tabindex="-1">DeepSeek 账号</h2>
        <button type="button" data-action="close" aria-label="关闭设置">×</button>
      </header>
      <p role="status" data-part="status"></p>
      <div data-part="identity" class="account-identity" hidden>
        <span class="account-avatar" data-part="avatar" aria-hidden="true"></span>
        <div><strong data-part="name"></strong><span data-part="contact" hidden></span></div>
      </div>
      <dl data-part="wallets" class="account-wallets" hidden>
        <div><dt>充值余额</dt><dd data-part="normal"></dd></div>
        <div data-part="bonus-row" hidden><dt>赠送余额</dt><dd data-part="bonus"></dd></div>
      </dl>
      <p role="alert" data-part="error" hidden></p>
      <div class="account-actions" aria-label="账号操作">
        <button type="button" data-action="sign-in">登录 DeepSeek 账号</button>
        <button type="button" data-action="open-browser" hidden>在浏览器中打开</button>
        <button type="button" data-action="copy-link" hidden>复制授权链接</button>
        <button type="button" data-action="cancel" hidden>取消登录</button>
        <button type="button" data-action="refresh" hidden>刷新</button>
        <button type="button" data-action="usage" hidden>用量</button>
        <button type="button" data-action="top-up" hidden>充值</button>
        <button type="button" data-action="check-signout" hidden>退出登录</button>
        <button type="button" data-action="back" hidden>返回</button>
        <button type="button" data-action="api-key" hidden>添加 API Key</button>
        <button type="button" data-action="continue" hidden>继续</button>
      </div>
      <p role="status" data-part="copy-result" hidden></p>
    </div>
    <div class="account-confirm" hidden>
      <section role="alertdialog" aria-modal="true" aria-labelledby="deepseek-account-signout" aria-describedby="deepseek-account-signout-message">
        <h3 id="deepseek-account-signout">退出 DeepSeek 账号</h3>
        <p id="deepseek-account-signout-message"></p>
        <p role="alert" data-part="confirm-error" hidden></p>
        <div class="account-actions">
          <button type="button" data-action="cancel-signout">取消</button>
          <button type="button" data-action="signout">确认退出</button>
        </div>
      </section>
    </div>`;
  const actions = Object.fromEntries([...root.querySelectorAll('[data-action]')].map(node => [node.dataset.action, node]));
  const parts = Object.fromEntries([...root.querySelectorAll('[data-part]')].map(node => [node.dataset.part, node]));
  const content = root.querySelector('.account-content');
  const confirm = root.querySelector('.account-confirm');
  const heading = root.querySelector('h2');
  controls = { root, actions, parts, content, confirm, heading };
  for (const [action, button] of Object.entries(actions)) {
    button.onclick = () => dispatch?.(`account-${action}`, '');
  }
  actions['copy-link'].onclick = async () => {
    const attempt = state?.state?.attempt;
    if (!attempt?.authorizeUrl) return;
    const href = account_authorization_url(attempt.authorizeUrl, state.dark);
    let copied = false;
    try {
      await navigator.clipboard.writeText(href);
      copied = true;
    } catch {}
    if (state?.state?.attempt?.id !== attempt.id || state?.state?.attempt?.authorizeUrl !== attempt.authorizeUrl) return;
    clearTimeout(copyTimer);
    parts['copy-result'].hidden = false;
    parts['copy-result'].textContent = copied ? '已复制授权链接。' : '无法复制授权链接。';
    copyTimer = setTimeout(() => { parts['copy-result'].hidden = true; }, 2000);
  };
  root.addEventListener('keydown', event => {
    if (event.isComposing || !state) return;
    const confirming = state.confirmSignout != null;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      const action = confirming ? 'cancel-signout' : state.onboarding ? 'back' : 'close';
      if (!event.repeat) dispatch?.(`account-${action}`, '');
    } else if (event.key === 'Tab' && (state.onboarding || state.launcherDialog || confirming)) {
      const items = [...(confirming ? confirm : content).querySelectorAll('button')]
        .filter(button => !button.disabled && button.getClientRects().length > 0);
      const boundary = event.shiftKey ? items[0] : items.at(-1);
      if (boundary && (document.activeElement === boundary || !items.includes(document.activeElement))) {
        event.preventDefault();
        (event.shiftKey ? items.at(-1) : items[0]).focus();
      }
    }
  });
  document.body.append(root);
  paintAvatar(parts.avatar);
}

const phases = {
  initializing: '正在准备浏览器登录…',
  'waiting-browser': '请在浏览器中完成登录。',
  exchanging: '正在确认授权…', committing: '正在保存账号凭据…',
  succeeded: '已登录 DeepSeek。', cancelled: '登录已取消。',
  expired: '登录已过期，请重新开始。', failed: '登录失败，请重试。',
};

const runningPhases = new Set(['initializing', 'waiting-browser', 'exchanging', 'committing']);
const loginErrors = { network: '无法连接 DeepSeek，请重试。', 'no-response': 'DeepSeek 暂未响应，请重试。', storage: '无法保存账号凭据，请重试。' };

function displayMoney(wallets) {
  return wallets?.map(wallet => `${wallet.currency === 'CNY' ? '¥' : '$'}${wallet.balance}`).join(' · ');
}

export function update_deepseek_account_form(next) {
  const previous = state;
  state = next;
  if (!next) {
    if (controls) controls.root.hidden = true;
    return;
  }
  if (!controls) mount();
  const { root, actions, parts, content, confirm, heading } = controls;
  const signedIn = next.state?.status === 'credential-stored';
  const attempt = next.state?.attempt;
  const running = runningPhases.has(attempt?.phase) || next.busy === 'start';
  const confirming = next.confirmSignout != null;
  const menuLogin = next.launcherDialog === 'sign-in';
  const menuLogout = next.launcherDialog === 'sign-out';
  const modal = next.onboarding || Boolean(next.launcherDialog);
  if (previous?.state?.attempt?.id !== attempt?.id || previous?.state?.attempt?.authorizeUrl !== attempt?.authorizeUrl) {
    clearTimeout(copyTimer);
    parts['copy-result'].hidden = true;
  }
  root.hidden = false;
  root.dataset.appearance = next.dark ? 'dark' : 'light';
  root.classList.toggle('account-modal', modal);
  root.setAttribute('role', menuLogout && confirming ? 'presentation' : modal ? 'dialog' : 'region');
  if (modal && !(menuLogout && confirming)) root.setAttribute('aria-modal', 'true');
  else root.removeAttribute('aria-modal');
  place();

  const phase = signedIn && attempt?.phase !== 'committing' ? 'succeeded'
    : running && next.busy === 'start' ? 'initializing' : attempt?.phase;
  heading.textContent = menuLogin ? '登录 DeepSeek 账号' : menuLogout ? '退出 DeepSeek 账号' : 'DeepSeek 账号';
  parts.status.textContent = menuLogout ? '正在检查运行中的任务…'
    : (phase === 'failed' ? loginErrors[attempt?.errorCode] : undefined) ?? phases[phase]
      ?? (next.state ? '请登录 DeepSeek 账号。' : '正在读取账号状态…');
  const profile = next.details?.profile?.value;
  const balance = next.details?.balance;
  parts.identity.hidden = !signedIn || modal;
  parts.name.textContent = profile?.name ?? (next.busy === 'details' ? '正在读取账号资料…' : 'DeepSeek 账号');
  parts.contact.hidden = !profile?.contact;
  parts.contact.textContent = profile?.contact ?? '';
  parts.wallets.hidden = !signedIn || modal;
  parts.normal.textContent = displayMoney(balance?.value) || (next.busy === 'details' ? '正在读取…' : '暂无法读取余额');
  const bonus = balance?.bonusWallets.filter(wallet => Number(wallet.balance) > 0);
  parts['bonus-row'].hidden = !bonus?.length;
  parts.bonus.textContent = displayMoney(bonus) ?? '';

  actions.close.hidden = next.onboarding;
  actions.close.setAttribute('aria-label', menuLogin ? '关闭登录' : menuLogout ? '关闭退出' : '关闭设置');
  actions.close.disabled = Boolean(menuLogout || menuLogin && (next.busy || phase === 'committing'));
  actions['sign-in'].hidden = menuLogout || signedIn || running || !next.state;
  actions['sign-in'].textContent = next.error || ['failed', 'expired'].includes(phase) ? '重试登录' : '登录 DeepSeek 账号';
  actions['open-browser'].hidden = !attempt?.authorizeUrl || !running || signedIn || menuLogout;
  actions['copy-link'].hidden = actions['open-browser'].hidden || !modal;
  actions.cancel.hidden = !running || attempt?.phase === 'committing';
  actions.refresh.hidden = modal || (!signedIn && !next.error);
  actions.refresh.textContent = next.busy === 'details' ? '刷新中…' : '刷新';
  actions.usage.hidden = !signedIn || modal || !next.state.links.usageUrl;
  actions['top-up'].hidden = !signedIn || modal || !next.state.links.topUpUrl;
  actions['check-signout'].hidden = !signedIn || modal;
  actions.back.hidden = !next.onboarding;
  actions['api-key'].hidden = !menuLogin || running;
  actions.continue.hidden = !next.onboarding || !signedIn;
  for (const action of ['sign-in', 'cancel', 'check-signout', 'signout', 'cancel-signout']) {
    actions[action].disabled = Boolean(next.busy && next.busy !== 'details');
  }
  actions.refresh.disabled = Boolean(next.busy);
  parts.error.hidden = !next.error || confirming;
  parts.error.textContent = next.error ?? '';
  parts['confirm-error'].hidden = !next.error || !confirming;
  parts['confirm-error'].textContent = next.error ?? '';
  confirm.hidden = !confirming;
  content.hidden = menuLogout && confirming;
  content.inert = confirming;
  const impacts = {
    running: '正在使用账号模型的任务会被停止。确认退出 DeepSeek 账号？',
    unknown: '暂时无法确认任务状态。退出可能停止正在使用账号模型的任务。',
    idle: '确认退出当前 DeepSeek 账号？',
  };
  confirm.querySelector('#deepseek-account-signout-message').textContent = impacts[next.confirmSignout] ?? '';
  if (confirming && previous?.confirmSignout == null) actions['cancel-signout'].focus();
  else if (!confirming && previous?.confirmSignout != null) (signedIn ? actions['check-signout'] : actions['sign-in']).focus();
  else if (!previous) heading.focus({ preventScroll: true });
  else if (next.onboarding && signedIn && previous.state?.status !== 'credential-stored') actions.continue.focus();
}
