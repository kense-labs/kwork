import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';

export async function installUploadMock(page) {
  await page.addInitScript(() => {
    const state = window.__uploadTest = { enabled: false, revision: 1, pending: [], readonly: false };
    const snapshot = () => ({ available: true, status: 'ready', writable: !state.readonly,
      revision: state.revision, value: { enabled: state.enabled } });
    window.__uploadRemote = request => request.operation === 'read' ? Promise.resolve(snapshot())
      : new Promise((resolve, reject) => state.pending.push({ request, reject, resolve: () => {
        state.enabled = request.enabled; state.revision++; resolve(snapshot());
      } }));
  });
}

export async function runUploadCase(page, artifacts) {
  page.setDefaultTimeout(12000);
  const shell = page.locator('#settings-shell');
  const row = page.locator('#settings-upload');
  const toggle = row.getByRole('switch', { name: '在使用官方模型 API 时上传 Session Log', exact: true });
  const close = shell.getByRole('button', { name: '关闭设置', exact: true });
  const open = async () => {
    await page.getByRole('button', { name: '更多设置', exact: true }).dispatchEvent('click');
    await page.getByRole('button', { name: '设置', exact: true }).dispatchEvent('click');
    await shell.getByRole('button', { name: '通用设置', exact: true }).click();
    await toggle.waitFor();
  };
  const reveal = async () => {
    await toggle.focus();
    await page.waitForFunction(() => {
      const control = document.querySelector('#settings-upload [role=switch]').getBoundingClientRect();
      const panel = document.querySelector('#settings-shell').getBoundingClientRect();
      return control.top >= panel.top + 54 && control.bottom <= panel.bottom;
    });
  };
  const pending = () => page.waitForFunction(() => window.__uploadTest.pending.length > 0);
  const settle = failure => page.evaluate(failure => {
    const write = window.__uploadTest.pending.shift();
    if (failure) write.reject(new Error('fixture refused')); else write.resolve();
  }, failure);
  await open();
  for (const label of ['网页搜索', '会话日志', 'API Key', '模型服务']) {
    assert.equal(await shell.getByRole('navigation').getByRole('button', { name: label, exact: true }).count(), 0);
  }
  await reveal();
  assert.equal(await toggle.getAttribute('aria-checked'), 'false');
  await page.screenshot({ path: join(artifacts, 'upload-row.png') });
  await toggle.click(); await pending();
  assert.equal(await toggle.isDisabled(), true);
  assert.equal(await toggle.getAttribute('aria-checked'), 'false', 'accepted value stays visible while saving');
  await toggle.dispatchEvent('click');
  assert.equal(await page.evaluate(() => window.__uploadTest.pending.length), 1);
  await close.click(); await shell.waitFor({ state: 'hidden' });
  await settle(false);
  await page.getByRole('alert').filter({ hasText: '设置已保存' }).waitFor();
  await page.screenshot({ path: join(artifacts, 'upload-saved-after-close.png') });
  await open(); await reveal();
  assert.equal(await toggle.getAttribute('aria-checked'), 'true');
  await toggle.click(); await pending(); await settle(true);
  await page.getByRole('alert').filter({ hasText: '无法保存设置' }).waitFor();
  assert.equal(await toggle.getAttribute('aria-checked'), 'true', 'failed write retains Host value');
  await page.evaluate(() => { window.__uploadTest.enabled = false; window.__uploadTest.revision++;
    window.__verification.emit({ kind: 'session_log_preference_changed' }); });
  await page.waitForFunction(() => document.querySelector('#settings-upload [role=switch]').getAttribute('aria-checked') === 'false');
  await page.setViewportSize({ width: 620, height: 820 });
  await reveal();
  await page.screenshot({ path: join(artifacts, 'upload-narrow.png') });
  const bounds = await row.boundingBox();
  assert.ok(bounds.width < 400 && bounds.height > 70, 'official row wraps in the Mac narrow pane');
  await page.evaluate(() => { window.__uploadTest.readonly = true;
    window.__verification.emit({ kind: 'session_log_preference_changed' }); });
  await page.waitForFunction(() => document.querySelector('#settings-upload [role=switch]').disabled);
  await close.focus(); await page.keyboard.press('Escape');
  await shell.waitFor({ state: 'hidden' });
  await writeFile(join(artifacts, 'upload-checks.json'), JSON.stringify({ passed: true,
    checks: ['official root navigation', 'Host accepted state', 'single write', 'notice after panel closes', 'failure retains accepted value',
      'external Host change', 'narrow wrapping', 'native focus scrolling', 'read-only state', 'Escape'] }, null, 2));
}
