import { acquireSurfaceBackground } from '/overlay-inert.js';

let dispatch;
let state;
let controls;
let step;
let dialog;
let dialogReturn;

export function listen_desktop_onboarding(callback) {
  dispatch = callback;
}

const copy = {
  welcome: ['欢迎使用 KWork', '选择本地文件夹作为工作区，交给 DeepSeek 查资料、处理文档、编写代码或排查问题。'],
  credit: ['准备可用额度', '只有智能体执行任务时才会消耗额度。请准备足够余额，避免任务中途停止。'],
  purpose: ['你希望 KWork 帮你做什么？', '选择你常做的工作。可以同时选择两项。'],
  process: ['你希望看到多少工作过程？', '选择对话中显示的工作步骤和细节。'],
};

const purpose = [
  ['office', '办公与创作', '编辑文档、整理数据、制作演示文稿', 'office'],
  ['development', '代码与开发', '编写代码、运行命令、调试和管理项目', 'code'],
];
const process = [
  ['compact', '聚焦结果', '只看结果，保持界面简洁', 'compact'],
  ['standard', '关键细节', '显示重要步骤和关键操作', 'standard'],
  ['detailed', '完整过程', '查看完整步骤，便于调试和排查', 'detailed'],
];

const button = (action, label, primary = false) =>
  `<button type="button" data-action="${action}"${primary ? ' class="onboarding-primary"' : ''}>${label}</button>`;
const card = ([value, title, description, icon], type, name) => `
  <label class="onboarding-choice">
    <span class="onboarding-choice-top"><img alt="" src="/onboarding/onboarding-${icon}.svg">
      <input type="${type}" name="${name}" value="${value}" aria-label="${title}"></span>
    <strong>${title}</strong><span>${description}</span>
  </label>`;

function artwork(name) {
  return `<div class="onboarding-artwork" aria-hidden="true">
    <img alt="" data-art="light" src="/onboarding/onboarding-${name}-zh.webp">
    <img alt="" data-art="dark" src="/onboarding/onboarding-${name}-zh-dark.webp">
  </div>`;
}

function renderStep(next) {
  const [title, description] = copy[next];
  const intro = `<div class="onboarding-copy">
    <h1 id="desktop-onboarding-title" tabindex="-1">${title}</h1><p>${description}</p></div>`;
  let content;
  if (next === 'welcome') {
    content = `${artwork('welcome')}${intro}<div class="onboarding-actions">${button('next', '开始设置', true)}</div>`;
  } else if (next === 'credit') {
    content = `${artwork('recharge')}${intro}<div class="onboarding-actions">
      ${button('credit-primary', '', true)}${button('credit-secondary', '')}</div>`;
  } else {
    const isPurpose = next === 'purpose';
    const choices = (isPurpose ? purpose : process)
      .map(entry => card(entry, isPurpose ? 'checkbox' : 'radio', next)).join('');
    content = `${intro}<div class="onboarding-choices" role="group" aria-label="${isPurpose ? '工作用途' : '工作过程'}">${choices}</div>
      <div class="onboarding-actions">${button(isPurpose ? 'next' : 'finish', isPurpose ? '继续' : '进入应用', true)}</div>`;
  }
  controls.content.dataset.step = next;
  controls.content.innerHTML = content;
  controls.nav.hidden = next === 'welcome';
}

function closeConfirmation(restore = true) {
  if (!dialog) return;
  controls.confirm.hidden = true;
  controls.page.inert = false;
  dialog = undefined;
  if (restore && dialogReturn?.isConnected) dialogReturn.focus({ preventScroll: true });
  dialogReturn = undefined;
}

function openConfirmation(mode) {
  const credit = mode !== 'skip';
  dialog = mode;
  dialogReturn = document.activeElement;
  controls.confirm.querySelector('h2').textContent = credit ? '暂时跳过充值？' : '跳过设置？';
  controls.confirm.querySelector('p').textContent = credit
    ? '没有可用额度时，智能体无法开始新任务。你可以稍后在账号设置中充值。'
    : '你可以稍后在通用设置中调整工作过程、性能与用量和代码工作工具。';
  controls.confirm.querySelector('[data-action="confirm-other"]').textContent = credit ? '我知道了' : '继续设置';
  const accept = controls.confirm.querySelector('[data-action="confirm-main"]');
  accept.textContent = credit ? '前往充值' : '进入应用';
  accept.disabled = credit && !state.canRecharge;
  controls.confirm.hidden = false;
  controls.page.inert = true;
  controls.confirm.querySelector('[data-action="confirm-other"]').focus({ preventScroll: true });
}

function act(action) {
  if (!state || state.loading || state.saving) return;
  if (state.mustRead && action !== 'retry') return;
  if (action === 'dismiss') {
    closeConfirmation();
  } else if (action === 'confirm-main') {
    const mode = dialog;
    closeConfirmation();
    dispatch?.(mode === 'skip' ? 'onboarding-skip' : 'onboarding-recharge', '');
  } else if (action === 'confirm-other') {
    const mode = dialog;
    closeConfirmation();
    if (mode !== 'skip') dispatch?.(mode === 'credit-skip' ? 'onboarding-skip' : 'onboarding-next', '');
  } else if (action === 'skip-confirmation') {
    openConfirmation(step === 'credit' ? 'credit-skip' : 'skip');
  } else if (action === 'credit-primary' || action === 'credit-secondary') {
    const proceed = (action === 'credit-primary') === state.fundedAtEntry;
    if (!proceed) dispatch?.('onboarding-recharge', '');
    else if (!state.fundedAtEntry && state.balanceFunded === false) openConfirmation('credit');
    else dispatch?.('onboarding-next', '');
  } else {
    dispatch?.(`onboarding-${action}`, '');
  }
}

function mount() {
  const root = document.createElement('section');
  root.id = 'desktop-onboarding';
  root.setAttribute('role', 'dialog');
  root.setAttribute('aria-modal', 'true');
  root.setAttribute('aria-labelledby', 'desktop-onboarding-title');
  root.innerHTML = `
    <div data-part="page">
      <div data-part="content"></div>
      <div data-part="error" role="alert" hidden><span></span>${button('retry', '重试')}</div>
      <footer data-part="nav" hidden>${button('back', '‹ 上一步')}${button('skip-confirmation', '跳过')}</footer>
    </div>
    <div data-part="confirm" hidden>
      <section role="alertdialog" aria-modal="true" aria-labelledby="onboarding-confirm-title" aria-describedby="onboarding-confirm-message">
        <header><h2 id="onboarding-confirm-title"></h2><button type="button" data-action="dismiss" aria-label="关闭">×</button></header>
        <p id="onboarding-confirm-message"></p>
        <div class="onboarding-actions">${button('confirm-other', '')}${button('confirm-main', '', true)}</div>
      </section>
    </div>`;
  const focus = document.activeElement;
  const parts = Object.fromEntries([...root.querySelectorAll('[data-part]')].map(node => [node.dataset.part, node]));
  root.addEventListener('click', event => {
    const target = event.target.closest('button[data-action]');
    if (target) act(target.dataset.action);
  });
  root.addEventListener('change', event => {
    const input = event.target;
    if (!(input instanceof HTMLInputElement) || state.saving || state.loading || state.mustRead) return;
    if (input.name === 'purpose') {
      const selected = [...parts.content.querySelectorAll('input:checked')];
      dispatch?.('onboarding-purpose', selected.length === 2 ? 'both' : selected[0]?.value ?? '');
    } else if (input.name === 'process' && input.checked) dispatch?.('onboarding-process', input.value);
  });
  root.addEventListener('keydown', event => {
    if (event.isComposing) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      if (dialog && !state.saving) closeConfirmation();
    } else if (event.key === 'Tab') {
      const items = [...(dialog ? parts.confirm : parts.page).querySelectorAll('button, input')]
        .filter(item => !item.disabled && item.getClientRects().length);
      const boundary = event.shiftKey ? items[0] : items.at(-1);
      if (!items.length || document.activeElement === boundary) {
        event.preventDefault();
        (event.shiftKey ? items.at(-1) : items[0])?.focus();
      }
    }
  });
  document.body.append(root);
  return { root, focus, release: acquireSurfaceBackground(root, [
    'account-platform-view', 'account-quota-dialog', 'account-quota-toast',
  ]), ...parts };
}

export function update_desktop_onboarding(next) {
  state = next;
  if (!next) {
    if (controls) {
      closeConfirmation(false);
      controls.release();
      controls.root.remove();
      if (controls.focus?.isConnected) controls.focus.focus({ preventScroll: true });
      controls = undefined;
      step = undefined;
    }
    return;
  }
  controls ??= mount();
  const { root, page, content, nav, error } = controls;
  root.dataset.appearance = next.dark ? 'dark' : 'light';
  const show = next.loading ? 'loading' : next.progress.step;
  if (show !== step) {
    closeConfirmation(false);
    step = show;
    if (show === 'loading') {
      content.dataset.step = show;
      content.innerHTML = '<h1 id="desktop-onboarding-title" tabindex="-1" role="status">正在加载设置…</h1>';
      nav.hidden = true;
    } else renderStep(step);
    content.querySelector('h1')?.focus({ preventScroll: true });
  }
  page.setAttribute('aria-busy', String(next.saving));
  const blocked = next.loading || next.saving;
  for (const item of page.querySelectorAll('input, button')) item.disabled = blocked || next.mustRead;
  const nextButton = content.querySelector('.onboarding-primary');
  if (step === 'credit') {
    const secondary = content.querySelector('[data-action="credit-secondary"]');
    nextButton.textContent = next.fundedAtEntry ? '继续' : '去充值';
    secondary.textContent = next.fundedAtEntry ? '去充值' : '下一步';
    (next.fundedAtEntry ? secondary : nextButton).disabled = blocked || next.mustRead || !next.canRecharge;
  } else if (step === 'purpose' || step === 'process') {
    for (const item of content.querySelectorAll('input')) {
      item.checked = step === 'purpose'
        ? next.progress.purpose === 'both' || next.progress.purpose === item.value
        : next.progress.process === item.value;
    }
    nextButton.hidden = !next.progress[step];
  }
  error.hidden = !next.error;
  error.querySelector('span').textContent = next.error ?? '';
  error.querySelector('button').disabled = blocked;
  error.querySelector('button').hidden = !next.canRetry;
}
