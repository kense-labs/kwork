// Lifecycle matches DeepSeek Harness's account notification controller.
const MAX_DELAY = 2_147_483_647;

function expiry(notice) {
  const value = notice.displayUntil == null ? NaN : Date.parse(notice.displayUntil);
  return Number.isNaN(value) ? Infinity : value;
}

export function createNotificationController({ read, readContent, report, publish, clock = globalThis, now = Date.now }) {
  let owner;
  let poll;
  let expiryTimer;
  let locale = 'zh-CN';
  let contentRevision = 0;
  let current = [];

  function scheduleExpiry() {
    if (expiryTimer !== undefined) clock.clearTimeout(expiryTimer);
    const deadline = current.reduce((first, entry) => Math.min(first, expiry(entry.notice)), Infinity);
    expiryTimer = Number.isFinite(deadline) ? clock.setTimeout(() => {
      expiryTimer = undefined;
      if (!removeExpired()) scheduleExpiry();
    }, Math.min(MAX_DELAY, Math.max(1, deadline - now()))) : undefined;
  }

  function render() {
    scheduleExpiry();
    publish(current.map(({ notice }) => notice));
  }

  function removeExpired() {
    const at = now();
    const remaining = current.filter(entry => expiry(entry.notice) > at);
    if (remaining.length !== current.length) {
      current = remaining;
      render();
      return true;
    }
    return false;
  }

  function apply(batch, requestedLocale) {
    const previous = new Map(current.map(entry => [entry.notice.id, entry]));
    const front = current[0]?.displayed ? current[0] : undefined;
    const next = new Map();
    for (const notice of batch.notifications) {
      if (next.has(notice.id)) continue;
      const entry = previous.get(notice.id) ?? { identity: {}, displayed: false };
      const old = entry.notice;
      entry.notice = requestedLocale !== locale && old
        ? { ...notice, title: old.title, body: old.body, actions: old.actions } : notice;
      if (expiry(entry.notice) > now()) next.set(notice.id, entry);
    }
    const retained = front && expiry(front.notice) > now();
    if (retained) next.delete(front.notice.id);
    current = retained ? [front, ...next.values()] : [...next.values()];
    render();
  }

  async function refreshContent() {
    const session = owner;
    if (session?.mode !== 'modern' || current.length === 0) return;
    const revision = ++contentRevision;
    const language = locale;
    const results = await Promise.allSettled(current.map(async ({ identity, notice }) => ({
      identity, copy: await readContent(session.scope, notice.id, language),
    })));
    if (owner !== session || contentRevision !== revision) return;
    const updates = new Map();
    for (const result of results) {
      if (result.status === 'rejected') {
        console.warn('无法刷新账号通知文案。');
      } else if (result.value.copy) {
        updates.set(result.value.identity, result.value.copy);
      }
    }
    for (const entry of current) {
      const copy = updates.get(entry.identity);
      if (copy?.id !== entry.notice.id) continue;
      entry.notice = { ...entry.notice, title: copy.title, body: copy.body, actions: copy.actions };
    }
    render();
  }

  async function readOnce(cycle) {
    const language = locale;
    let batch;
    try { batch = await read(cycle.session.scope, language); }
    catch {
      if (poll === cycle) console.warn('账号通知读取失败。');
    }
    if (poll !== cycle) return;
    if (batch) {
      const delay = batch.nextPollAfterSeconds * 1000;
      if (Number.isFinite(delay) && delay > 0 && delay <= MAX_DELAY) cycle.delay = delay;
      apply(batch, language);
      if (language !== locale) await refreshContent();
    }
    if (poll === cycle) cycle.timer = clock.setTimeout(() => { void readOnce(cycle); }, cycle.delay);
  }

  function stopPoll() {
    if (poll?.timer !== undefined) clock.clearTimeout(poll.timer);
    poll = undefined;
  }

  function restart() {
    if (!owner) return;
    stopPoll();
    poll = { session: owner, delay: owner.pollIntervalMs };
    void readOnce(poll);
  }

  function send(id, event) {
    const session = owner;
    if (session.mode !== 'modern' && event !== 'displayed') return;
    void report(session.scope, id, event, locale).catch(() => {
      if (owner === session) console.warn('账号通知事件上报失败。', event);
    });
  }

  function front(id) {
    removeExpired();
    return owner && current[0]?.notice.id === id ? current[0] : undefined;
  }

  return {
    setScope(next) {
      if ((next?.scope ?? null) === (owner?.scope ?? null)) return;
      stopPoll();
      owner = next;
      contentRevision += 1;
      current = [];
      render();
      restart();
    },
    refresh: restart,
    setLocale(next) {
      if (locale === next) return;
      locale = next;
      if (owner?.mode === 'bonus') restart();
      else void refreshContent();
    },
    presented(id) {
      const entry = front(id);
      if (!entry || entry.displayed) return;
      entry.displayed = true;
      send(id, 'displayed');
    },
    click(id) {
      if (!front(id)) return false;
      send(id, 'cta_clicked');
      return true;
    },
    dismiss(id) {
      if (!front(id)) return false;
      send(id, 'dismissed');
      current.shift();
      render();
      return true;
    },
  };
}

let settings;
let geometry;
let covered;
let pendingCover;
let scopeController;
let root;
let action;
let cards = new Map();
let entryOnScreen;
let resetPresentation;
let immediatePaint;
let records = [];

const closeLabels = { zh: '关闭', en: 'Close', ja: '閉じる', es: 'Cerrar', fr: 'Fermer',
  de: 'Schließen', ru: 'Закрыть', pt: 'Fechar' };
const language = () => document.documentElement.lang || 'zh-CN';
const request = data => window.__TAURI__.core.invoke('account_notifications', {
  request: { ...data, timezoneOffsetSeconds: -new Date().getTimezoneOffset() * 60 },
});

function controller() {
  if (scopeController) return scopeController;
  scopeController = createNotificationController({
    read: (scope, locale) => request({ action: 'list', scope, locale }),
    readContent: (scope, id, locale) => request({ action: 'content', scope, id, locale }),
    report: (scope, id, event, locale) => request({ action: 'report', scope, id, event, locale }),
    publish: notices => {
      records = notices;
      updateCards();
    },
  });
  scopeController.setLocale(language());
  new MutationObserver(() => {
    scopeController.setLocale(language());
    updateCards();
  }).observe(document.documentElement, { attributes: true, attributeFilter: ['lang'] });
  return scopeController;
}

export function listen_account_notice_open(callback) {
  action = callback;
}

export function place_account_notices(left, top, width) {
  const positioned = Boolean(geometry);
  geometry = { left, top, width };
  if (root) {
    position();
    if (!positioned) updateCards();
    else updateCover();
  }
}

export function place_account_notice_cover(x, y, width, height) {
  covered = { x, y, width, height };
  updateCover();
}

function updateCover() {
  if (!root || pendingCover !== undefined) return;
  pendingCover = requestAnimationFrame(() => {
    pendingCover = undefined;
    let path = '';
    if (settings?.settingsOpen && covered && !root.hidden) {
      const rect = root.getBoundingClientRect();
      const left = Math.max(0, covered.x - rect.x);
      const top = Math.max(0, covered.y - rect.y);
      const right = Math.min(rect.width, covered.x + covered.width - rect.x);
      const bottom = Math.min(rect.height, covered.y + covered.height - rect.y);
      if (left < right && top < bottom) {
        path = `path(evenodd, "M0 -48 H${rect.width} V${rect.height} H0 Z M${left} ${top} H${right} V${bottom} H${left} Z")`;
      }
    }
    if (root.style.clipPath !== path) root.style.clipPath = path;
  });
}

function position() {
  if (!geometry) return;
  root.style.left = `${geometry.left + 2}px`;
  root.style.top = `${geometry.top - 5}px`;
  root.style.width = `${Math.min(280, Math.max(0, geometry.width - 4))}px`;
}

export function update_account_notices(next) {
  const previous = settings;
  settings = next;
  if (!next?.feed && !scopeController) return;
  const active = controller();
  const changed = previous?.feed?.scope !== next?.feed?.scope;
  if (changed) active.setScope(next?.feed);
  else if (next?.feed && ((!previous?.settingsOpen && next.settingsOpen) || next.refresh !== previous?.refresh)) {
    active.refresh();
  }
  updateCards();
}

function mount() {
  root = document.createElement('div');
  root.id = 'account-notifications';
  root.hidden = true;
  document.body.append(root);
  position();
}

function dismiss(card, id) {
  if (!settings?.interactive) return;
  const focused = card.contains(document.activeElement);
  const next = card.nextElementSibling;
  const previous = next?.getBoundingClientRect();
  if (!scopeController.dismiss(id)) return;
  if (previous && next.isConnected && next.animate && !matchMedia('(prefers-reduced-motion: reduce)').matches) {
    const after = next.getBoundingClientRect();
    if (after.width && after.height) next.animate([
      { transform: `translate(${previous.left - after.left}px, ${previous.top - after.top}px) scale(${previous.width / after.width}, ${previous.height / after.height})` },
      { transform: 'none' },
    ], { duration: 250, easing: 'ease-out' });
  }
  if (focused) (root.querySelector('[data-front="true"] [data-close]') ??
    document.querySelector('#deepseek-account-launcher > button'))?.focus({ preventScroll: true });
}

function newCard(id) {
  const card = document.createElement('aside');
  card.innerHTML = `<div role="status"><strong></strong><p></p></div>
    <div class="account-notice-actions"></div><button type="button" data-close>×</button>`;
  const close = card.querySelector('[data-close]');
  close.onclick = () => dismiss(card, id);
  return { card, close, title: card.querySelector('strong'), body: card.querySelector('p'),
    actions: card.querySelector('.account-notice-actions'), buttons: new Map(), presented: false };
}

function renderCard(entry, notice, depth) {
  const front = depth === 0;
  const interactive = front && settings.interactive;
  entry.card.dataset.front = String(front);
  entry.card.setAttribute('aria-hidden', String(!interactive));
  entry.card.style.setProperty('--notice-depth', depth);
  entry.card.style.setProperty('--notice-scale', (252 / 260) ** depth);
  if (entry.title.textContent !== notice.title) entry.title.textContent = notice.title;
  if (entry.body.textContent !== notice.body) entry.body.textContent = notice.body;
  const close = closeLabels[language().split(/[-_]/)[0]] ?? closeLabels.en;
  entry.close.setAttribute('aria-label', close);
  entry.close.tabIndex = interactive ? 0 : -1;
  const old = entry.buttons;
  const used = new Map();
  const counts = new Map();
  for (const item of notice.actions) {
    const index = counts.get(item.target) ?? 0;
    counts.set(item.target, index + 1);
    const key = `${item.target}:${index}`;
    const button = old.get(key) ?? document.createElement('button');
    button.type = 'button';
    button.dataset.variant = item.style;
    if (button.textContent !== item.label) button.textContent = item.label;
    button.tabIndex = interactive ? 0 : -1;
    if (!button.isConnected) entry.actions.append(button);
    button.onclick = () => {
      if (entry.card.dataset.front !== 'true' || !settings?.interactive) return;
      if (item.target === 'dismiss') dismiss(entry.card, notice.id);
      else if (scopeController.click(notice.id)) action?.('account-notice-open', '');
    };
    used.set(key, button);
  }
  for (const [key, button] of old) {
    if (!used.has(key)) button.remove();
  }
  const ordered = [...used.values()];
  ordered.forEach((button, index) => {
    if (entry.actions.children[index] !== button) entry.actions.insertBefore(button, entry.actions.children[index] ?? null);
  });
  entry.buttons = used;
  entry.actions.hidden = !used.size;
}

function observeFront(notice, entry) {
  if (entryOnScreen === entry && immediatePaint === notice.displayUntil) return;
  resetPresentation?.();
  entryOnScreen = entry;
  immediatePaint = notice.displayUntil;
  if (entry.presented) return;
  let intersecting = false;
  let frame;
  let timer;
  const visible = () => intersecting && document.visibilityState !== 'hidden'
    && !entry.card.closest('[hidden], [inert]') && expiry(notice) > Date.now();
  const schedule = () => {
    if (entry.presented || frame !== undefined || timer !== undefined || !visible()) return;
    frame = requestAnimationFrame(() => {
      frame = undefined;
      timer = setTimeout(() => {
        timer = undefined;
        if (!visible()) return;
        entry.presented = true;
        scopeController.presented(notice.id);
      }, 0);
    });
  };
  const observer = new IntersectionObserver(([record]) => {
    intersecting = record.isIntersecting;
    schedule();
  });
  observer.observe(entry.card);
  document.addEventListener('visibilitychange', schedule);
  const attributes = new MutationObserver(schedule);
  attributes.observe(root, { attributes: true, attributeFilter: ['hidden', 'inert'] });
  resetPresentation = () => {
    observer.disconnect();
    attributes.disconnect();
    document.removeEventListener('visibilitychange', schedule);
    if (frame !== undefined) cancelAnimationFrame(frame);
    if (timer !== undefined) clearTimeout(timer);
  };
}

function updateCards() {
  if (!root && records.length === 0) return;
  if (!root) mount();
  root.hidden = !settings?.visible || !geometry || records.length === 0;
  root.dataset.appearance = settings?.dark ? 'dark' : 'light';
  root.dataset.interactive = String(Boolean(settings?.interactive));
  if (!records.length) {
    for (const { card } of cards.values()) card.remove();
    cards.clear();
    resetPresentation?.();
    resetPresentation = entryOnScreen = undefined;
    return;
  }
  const current = new Map();
  records.forEach((notice, depth) => {
    const entry = cards.get(notice.id) ?? newCard(notice.id);
    renderCard(entry, notice, depth);
    if (root.children[depth] !== entry.card) root.insertBefore(entry.card, root.children[depth] ?? null);
    current.set(notice.id, entry);
  });
  for (const [id, entry] of cards) {
    if (!current.has(id)) entry.card.remove();
  }
  cards = current;
  observeFront(records[0], cards.get(records[0].id));
  updateCover();
}
