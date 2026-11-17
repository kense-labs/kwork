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

const closeIcon = '<svg aria-hidden="true" viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor"><path d="M2.5 2.5 13.5 13.5M13.5 2.5 2.5 13.5"/></svg>';
const externalIcon = '<svg aria-hidden="true" viewBox="0 0 16 16" width="12" height="12" fill="none"><path fill="currentColor" d="M11.7256 2.77441C12.5538 2.77469 13.2256 3.44616 13.2256 4.27441V10.1416H12.2256V4.27441C12.2256 3.99844 12.0015 3.77469 11.7256 3.77441H5.7207V2.77441H11.7256Z"/><path d="M2.77441 13.2255 12.3756 3.62427" stroke="currentColor"/></svg>';

export function account_authorization_url(url, dark) {
  const link = new URL(url);
  link.searchParams.set('theme', dark ? 'dark' : 'light');
  return link.href;
}

export function account_profile_url(usageUrl) {
  try { return new URL('/', usageUrl).href; }
  catch { return undefined; }
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
        <button type="button" class="account-close" data-action="close" aria-label="关闭设置">${closeIcon}</button>
      </header>
      <div class="account-signed-out" data-part="signed-out" hidden>
        <div><span>当前未登录 DeepSeek Harness 账号</span>
          <span role="status" data-part="signed-out-description"></span>
        </div>
      </div>
      <p role="status" data-part="status"></p>
      <div data-part="cards" class="account-cards" hidden>
        <div class="account-profile">
          <div class="account-identity">
            <span class="account-avatar" data-part="avatar" aria-hidden="true"></span>
            <div><strong data-part="name"></strong><span data-part="contact" role="status"></span></div>
          </div>
          <a class="account-profile-link" data-action="profile" target="_blank" rel="noreferrer" hidden>更多账号信息${externalIcon}</a>
        </div>
        <dl class="account-wallets">
          <div><dt>充值余额</dt><dd><span class="account-amount" data-part="normal"></span>
            <span data-part="balance-loading" hidden>加载中…</span>
            <a class="account-unavailable" data-action="balance-usage" hidden>前往开放平台查看</a>
          </dd></div>
          <div data-part="bonus-row" hidden><dt>赠金余额</dt><dd class="account-amount" data-part="bonus"></dd></div>
          <div data-part="balance-actions"><dt>更多</dt><dd class="account-balance-actions">
            <a class="account-outline" data-action="usage" hidden>查询用量</a>
            <a class="account-primary" data-action="top-up" hidden>充值</a>
          </dd></div>
        </dl>
      </div>
      <p role="alert" data-part="error" hidden></p>
      <div class="account-actions" aria-label="账号操作">
        <button type="button" data-action="sign-in">登录 DeepSeek 账号</button>
        <button type="button" data-action="open-browser" hidden>在浏览器中打开</button>
        <button type="button" data-action="copy-link" hidden>复制授权链接</button>
        <button type="button" data-action="cancel" hidden>取消登录</button>
        <button type="button" data-action="refresh" hidden>刷新</button>
        <button type="button" data-action="back" hidden>返回</button>
        <button type="button" data-action="api-key" hidden>添加 API Key</button>
        <button type="button" data-action="continue" hidden>继续</button>
      </div>
      <p role="status" data-part="copy-result" hidden></p>
    </div>
    <div class="account-confirm" hidden>
      <div class="account-confirm-mask" aria-hidden="true"></div>
      <section role="alertdialog" aria-modal="true" aria-labelledby="deepseek-account-signout" aria-describedby="deepseek-account-signout-message">
        <div class="account-confirm-content">
          <header><h3 id="deepseek-account-signout">退出登录</h3>
            <button type="button" class="account-close" data-action="close-signout" aria-label="关闭">${closeIcon}</button>
          </header>
          <p id="deepseek-account-signout-message"></p>
          <p role="alert" data-part="confirm-error" hidden></p>
        </div>
        <footer>
          <button type="button" class="account-outline" data-action="cancel-signout">取消</button>
          <button type="button" class="account-primary" data-action="signout">退出登录</button>
        </footer>
      </section>
    </div>`;
  const actions = Object.fromEntries([...root.querySelectorAll('[data-action]')].map(node => [node.dataset.action, node]));
  const parts = Object.fromEntries([...root.querySelectorAll('[data-part]')].map(node => [node.dataset.part, node]));
  const content = root.querySelector('.account-content');
  const confirm = root.querySelector('.account-confirm');
  const heading = root.querySelector('h2');
  controls = { root, actions, parts, content, confirm, heading };
  for (const [action, button] of Object.entries(actions)) {
    button.onclick = event => {
      if (button instanceof HTMLAnchorElement) event.preventDefault();
      dispatch?.(`account-${action === 'balance-usage' ? 'usage' : action}`, '');
    };
  }
  const dismissSignout = () => {
    if (!actions['cancel-signout'].disabled) dispatch?.('account-cancel-signout', '');
  };
  actions['close-signout'].onclick = dismissSignout;
  confirm.querySelector('.account-confirm-mask').onclick = dismissSignout;
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

function displayMoney({ balance, currency }) {
  const symbol = currency === 'CNY' ? '¥' : '$';
  const [, sign, integer, fraction = '', exponent = '0'] =
    /^(-?)(\d*)(?:\.(\d*))?(?:e([+-]?\d+))?$/i.exec(balance) ?? [];
  if (integer === undefined) return null;
  const rawDigits = integer + fraction;
  const significant = rawDigits.search(/[1-9]/);
  if (significant === -1) return `${symbol}0.00`;

  // Work in decimal digits so platforms' extra precision cannot round up a positive balance.
  const centPosition = integer.length + Number(exponent) + 2 - significant;
  if (centPosition <= 0) return `${sign || '<'}${symbol}0.01`;
  if (centPosition > 311) return null;
  const digits = rawDigits.slice(significant);
  let cents = BigInt(digits.slice(0, centPosition).padEnd(centPosition, '0'));
  if (sign && digits[centPosition] >= '5') cents += 1n;
  return `${sign}${symbol}${(cents / 100n).toLocaleString()}.${String(cents % 100n).padStart(2, '0')}`;
}

function paintMoney(holder, wallets) {
  const amounts = wallets?.map(displayMoney).filter(Boolean) ?? [];
  holder.replaceChildren(...amounts.map(amount => {
    const span = document.createElement('span');
    span.textContent = amount;
    return span;
  }));
  return amounts.length > 0;
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
  const settingsSignedOut = !modal && !signedIn && !running;
  if (previous?.state?.attempt?.id !== attempt?.id || previous?.state?.attempt?.authorizeUrl !== attempt?.authorizeUrl) {
    clearTimeout(copyTimer);
    parts['copy-result'].hidden = true;
  }
  root.hidden = false;
  root.dataset.appearance = next.dark ? 'dark' : 'light';
  root.classList.toggle('account-modal', modal);
  root.classList.toggle('account-signout-open', confirming);
  root.classList.toggle('account-settings', !modal);
  root.setAttribute('role', menuLogout && confirming ? 'presentation' : modal ? 'dialog' : 'region');
  if (modal && !(menuLogout && confirming)) root.setAttribute('aria-modal', 'true');
  else root.removeAttribute('aria-modal');
  place();

  const phase = signedIn && attempt?.phase !== 'committing' ? 'succeeded'
    : running && next.busy === 'start' ? 'initializing' : attempt?.phase;
  heading.textContent = menuLogin ? '登录 DeepSeek 账号' : menuLogout ? '退出登录'
    : next.onboarding ? 'DeepSeek 账号' : '账号与余额';
  parts.status.textContent = menuLogout ? '正在检查运行中的任务…'
    : (phase === 'failed' ? loginErrors[attempt?.errorCode] : undefined) ?? phases[phase]
      ?? (next.state ? '请登录 DeepSeek 账号。' : '正在读取账号状态…');
  const profile = next.details?.profile?.value;
  const balance = next.details?.balance;
  const usageUrl = next.state?.links?.usageUrl;
  const topUpUrl = next.state?.links?.topUpUrl;
  parts.cards.hidden = !signedIn || modal;
  parts.status.hidden = settingsSignedOut || signedIn && !modal;
  parts.name.textContent = profile?.name ?? '已登录 DeepSeek';
  parts.contact.textContent = profile?.contact ?? (next.details?.profile ? '账号资料暂不可用' : '加载中…');
  parts['signed-out'].hidden = !settingsSignedOut;
  parts['signed-out-description'].textContent = ['failed', 'expired'].includes(phase)
    ? parts.status.textContent : '登录 DeepSeek Harness 账号获取专属 API Key';
  const hasBalance = paintMoney(parts.normal, balance?.value);
  parts['balance-loading'].hidden = hasBalance || Boolean(balance);
  parts['bonus-row'].hidden = !paintMoney(parts.bonus, balance?.bonusWallets?.filter(wallet => Number(wallet.balance) > 0));
  for (const [action, url] of Object.entries({ usage: usageUrl, 'top-up': topUpUrl, 'balance-usage': usageUrl })) {
    actions[action].hidden = !url || (action === 'balance-usage' && (hasBalance || !balance));
    if (url) actions[action].href = url;
    else actions[action].removeAttribute('href');
  }
  parts['balance-actions'].hidden = !usageUrl && !topUpUrl;
  const profileUrl = account_profile_url(usageUrl);
  actions.profile.hidden = !profileUrl;
  if (profileUrl) actions.profile.href = profileUrl;
  else actions.profile.removeAttribute('href');
  const signInParent = settingsSignedOut ? parts['signed-out'] : content.querySelector('.account-actions');
  if (actions['sign-in'].parentElement !== signInParent) {
    if (settingsSignedOut) signInParent.append(actions['sign-in']);
    else signInParent.prepend(actions['sign-in']);
  }
  actions['sign-in'].classList.toggle('account-primary', settingsSignedOut);

  actions.close.hidden = next.onboarding || !modal;
  actions.close.setAttribute('aria-label', menuLogin ? '关闭登录' : menuLogout ? '关闭退出' : '关闭设置');
  actions.close.disabled = Boolean(menuLogout || menuLogin && (next.busy || phase === 'committing'));
  actions['sign-in'].hidden = menuLogout || signedIn || running || !next.state;
  actions['sign-in'].textContent = settingsSignedOut ? '登录'
    : next.error || ['failed', 'expired'].includes(phase) ? '重试登录' : '登录 DeepSeek 账号';
  actions['open-browser'].hidden = !attempt?.authorizeUrl || !running || signedIn || menuLogout;
  actions['copy-link'].hidden = actions['open-browser'].hidden || !modal;
  actions.cancel.hidden = !running || attempt?.phase === 'committing';
  actions.refresh.hidden = modal || signedIn || !next.error;
  actions.refresh.textContent = next.busy === 'details' ? '刷新中…' : '刷新';
  actions.back.hidden = !next.onboarding;
  actions['api-key'].hidden = !menuLogin || running;
  actions.continue.hidden = !next.onboarding || !signedIn;
  for (const action of ['sign-in', 'cancel', 'signout', 'cancel-signout', 'close-signout']) {
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
    running: '当前有进程正在执行中，点击“退出登录”，将会打断进程，是否即刻退出登录？',
    unknown: '暂时无法确认任务状态。退出登录可能会中断使用此账号的任务，是否继续？',
    idle: '退出登录不会丢失任何数据，你仍可以登录此账号。',
  };
  confirm.querySelector('#deepseek-account-signout-message').textContent = impacts[next.confirmSignout] ?? '';
  if (confirming && previous?.confirmSignout == null) actions['close-signout'].focus({ preventScroll: true });
  else if (!confirming && previous?.confirmSignout != null) actions.close.focus({ preventScroll: true });
  else if (!previous) heading.focus({ preventScroll: true });
  else if (next.onboarding && signedIn && previous.state?.status !== 'credential-stored') actions.continue.focus();
}
