let dispatch;
let dialog;
let toast;
let cancel;
let shown;
let timer;

export function listen_account_quota_notice(callback) {
  dispatch = callback;
}

function mountDialog() {
  dialog = document.createElement('dialog');
  dialog.id = 'account-quota-dialog';
  dialog.setAttribute('aria-labelledby', 'account-quota-title');
  dialog.setAttribute('aria-describedby', 'account-quota-description');
  dialog.innerHTML = `
    <header>
      <h2 id="account-quota-title">当前无可用余额</h2>
      <button type="button" data-quota-close aria-label="关闭">×</button>
    </header>
    <p id="account-quota-description">没有可用额度时，KWork 无法开始新的任务。你可以现在充值，也可以稍后前往“设置 → 账号与余额”充值。</p>
    <footer>
      <button type="button" data-quota-cancel autofocus>取消</button>
      <button type="button" data-quota-pay>去充值</button>
    </footer>`;
  const close = dialog.querySelector('[data-quota-close]');
  cancel = dialog.querySelector('[data-quota-cancel]');
  const pay = dialog.querySelector('[data-quota-pay]');
  const dismiss = () => dispatch?.('account-quota-dismiss', shown.id);
  close.onclick = cancel.onclick = dismiss;
  pay.onclick = () => dispatch?.('account-quota-top-up', shown.id);
  dialog.addEventListener('cancel', event => {
    event.preventDefault();
    dismiss();
  });
  dialog.addEventListener('click', event => {
    if (event.target === dialog) {
      const { left, right, top, bottom } = dialog.getBoundingClientRect();
      if (event.clientX < left || event.clientX > right || event.clientY < top || event.clientY > bottom) dismiss();
    }
  });
  dialog.addEventListener('keydown', event => {
    if (event.isComposing || event.key !== 'Tab') return;
    if (document.activeElement === (event.shiftKey ? close : pay)) {
      event.preventDefault();
      (event.shiftKey ? pay : close).focus();
    }
  });
  document.body.append(dialog);
}

function showToast(id) {
  toast = document.createElement('div');
  toast.id = 'account-quota-toast';
  toast.setAttribute('role', 'alert');
  toast.innerHTML = '<span aria-hidden="true">!</span>当前请求的额度已用尽';
  document.body.append(toast);
  timer = setTimeout(() => dispatch?.('account-quota-dismiss', id), 4000);
}

export function update_account_quota_notice(id, kind, dark) {
  if (dialog) dialog.dataset.appearance = dark ? 'dark' : 'light';
  if (shown?.id === id && shown.kind === kind) return;
  clearTimeout(timer);
  toast?.remove();
  toast = undefined;
  shown = { id, kind };
  if (kind === 'balance') {
    if (!dialog) mountDialog();
    dialog.dataset.appearance = dark ? 'dark' : 'light';
    if (!dialog.open) {
      dialog.showModal();
      cancel.focus({ preventScroll: true });
    }
  } else {
    if (dialog?.open) dialog.close();
    if (kind === 'toast') showToast(id);
  }
}
