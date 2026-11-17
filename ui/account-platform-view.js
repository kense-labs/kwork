import { acquireSurfaceBackground } from '/overlay-inert.js';

let dispatch;
let overlay;
let active;
let backdrop;
let retiring;
let nextId = 0;

export function listen_account_platform_view(callback) {
  dispatch = callback;
}

const invoke = (visit, request) => Promise.resolve().then(() =>
  window.__TAURI__.core.invoke('account_platform_view', { shell: visit.shell, request }));

function mount() {
  const root = document.createElement('section');
  root.id = 'account-platform-view';
  root.setAttribute('role', 'dialog');
  root.setAttribute('aria-modal', 'true');
  root.setAttribute('aria-labelledby', 'account-platform-title');
  root.dataset.mac = String(/Mac/.test(navigator.platform));
  root.hidden = true;
  root.innerHTML = `
    <header>
      <button type="button" data-part="back"><span aria-hidden="true">‹</span> 返回 KWork</button>
      <h2 id="account-platform-title"></h2>
    </header>
    <div data-part="viewport">
      <div data-part="status" role="status">
        <span data-part="message"></span>
        <button type="button" data-part="retry" hidden>重试</button>
      </div>
    </div>`;
  const controls = Object.fromEntries([...root.querySelectorAll('[data-part]')].map(node => [node.dataset.part, node]));
  controls.back.onclick = close_account_platform_view;
  controls.retry.onclick = () => { if (active?.status === 'failed') start(active); };
  root.querySelector('header').addEventListener('pointerdown', event => {
    if (event.button === 0 && !event.target.closest('button')) {
      void window.__TAURI__.core.invoke('start_window_dragging').catch(() => {});
    }
  });
  root.addEventListener('keydown', event => {
    if (event.isComposing || !active) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      if (!event.repeat) close_account_platform_view();
    } else if (event.key === 'Tab' && active.status !== 'loaded') {
      const ends = [controls.back, controls.retry].filter(node => !node.hidden && !node.disabled);
      if (ends.length && (event.shiftKey ? ends[0] : ends.at(-1)) === document.activeElement) {
        event.preventDefault();
        (event.shiftKey ? ends.at(-1) : ends[0]).focus();
      }
    }
  });
  document.body.append(root);
  overlay = { root, title: root.querySelector('h2'), ...controls };
}

function paint(visit) {
  const { status, message, back, retry } = overlay;
  status.hidden = visit.status === 'loaded';
  status.classList.toggle('account-platform-loading', visit.status === 'loading');
  message.textContent = visit.status === 'failed' ? '未能加载 DeepSeek 平台页面。' : '正在加载…';
  retry.hidden = visit.status !== 'failed';
  back.disabled = visit.closing;
  retry.disabled = visit.closing;
}

function bounds() {
  const { x, y, width, height } = overlay.viewport.getBoundingClientRect();
  return { x: Math.max(0, x), y: Math.max(0, y), width, height };
}

function fail(visit, id) {
  if (active !== visit || visit.id !== id || visit.closing || visit.status === 'failed') return;
  clearTimeout(visit.timeout);
  visit.status = 'failed';
  visit.readyForBounds = false;
  void invoke(visit, { action: 'close', id }).catch(() => {});
  paint(visit);
}

async function sendBounds(visit) {
  if (!visit.readyForBounds || visit.sentBounds || visit.closing || active !== visit) return;
  visit.sentBounds = true;
  const id = visit.id;
  try {
    while (active === visit && visit.id === id && visit.queuedBounds && !visit.closing && visit.readyForBounds) {
      visit.queuedBounds = false;
      await invoke(visit, { action: 'bounds', id, bounds: bounds() });
    }
  } catch {
    fail(visit, id);
  } finally {
    if (visit.id === id) visit.sentBounds = false;
  }
}

function start(visit) {
  clearTimeout(visit.timeout);
  const id = ++nextId;
  Object.assign(visit, { id, status: 'loading', closing: false, readyForBounds: false,
    sentBounds: false, queuedBounds: false });
  paint(visit);
  overlay.back.focus({ preventScroll: true });
  visit.timeout = setTimeout(() => fail(visit, id), 20000);
  const initial = bounds();
  void invoke(visit, { action: 'open', id, page: visit.page, bounds: initial }).then(() => {
    if (active !== visit || visit.id !== id || visit.closing || visit.status === 'failed') return;
    visit.readyForBounds = true;
    visit.queuedBounds = true;
    void sendBounds(visit);
  }, () => fail(visit, id));
}

function dispose(visit) {
  clearTimeout(visit.timeout);
  visit.resize.disconnect();
  active = undefined;
  retiring = visit;
  const release = () => {
    if (active || retiring !== visit) return;
    retiring = undefined;
    overlay.root.hidden = true;
    const { release, focus } = backdrop;
    backdrop = undefined;
    release();
    if (focus?.isConnected) focus.focus({ preventScroll: true });
  };
  if (visit.closed) release();
  else void invoke(visit, { action: 'close', id: visit.id }).then(release, release);
}

export function update_account_platform_view(page, dark, shell) {
  if (!page) {
    if (active) dispose(active);
    return;
  }
  if (!overlay) mount();
  overlay.root.dataset.appearance = dark ? 'dark' : 'light';
  if (active?.page === page) return;
  if (active) {
    // Keep a single overlay and return target when an owner replaces its page.
    active.page = page;
    overlay.title.textContent = page === 'usage' ? '用量' : '充值';
    start(active);
    return;
  }
  if (!backdrop) {
    const focus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    backdrop = { focus, release: acquireSurfaceBackground(overlay.root) };
  }
  overlay.root.hidden = false;
  overlay.title.textContent = page === 'usage' ? '用量' : '充值';
  active = { page, shell };
  const visit = active;
  visit.resize = new ResizeObserver(() => {
    visit.queuedBounds = true;
    void sendBounds(visit);
  });
  visit.resize.observe(overlay.viewport);
  start(visit);
}

export function close_account_platform_view() {
  const visit = active;
  if (!visit || visit.closing) return;
  visit.closing = true;
  paint(visit);
  const done = () => {
    if (active !== visit || visit.closed) return;
    visit.closed = true;
    dispatch?.('account-platform-return', '');
  };
  void invoke(visit, { action: 'close', id: visit.id }).then(done, done);
}

export function account_platform_status(id, status, shell) {
  const visit = active;
  if (!visit || visit.id !== id || visit.shell !== shell) return;
  if (status === 'revoked') {
    visit.closed = visit.closing = true;
    clearTimeout(visit.timeout);
    paint(visit);
    setTimeout(() => {
      if (active === visit && visit.id === id) dispatch?.('account-platform-revoked', '');
    }, 0);
    return;
  }
  if (visit.closing || visit.status === 'failed') return;
  if (status === 'failed') fail(visit, id);
  else if (status === 'loaded') {
    clearTimeout(visit.timeout);
    visit.status = 'loaded';
    paint(visit);
  }
}
