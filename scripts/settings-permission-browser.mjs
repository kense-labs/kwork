import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';

export async function installPermissionMock(page) {
  await page.addInitScript(() => {
    const state = window.__permissionTest = { preset: 'workspace-write', revision: 1, pending: [], readonly: false };
    const snapshot = () => ({ view: { writable: !state.readonly, namespaces: [{ ns: 'permission', revision: state.revision,
      value: { defaultPreset: state.preset } }] }, catalog: { defaultPreset: state.preset,
      defaultOptions: ['read-only', 'workspace-write', 'danger-full-access'].map(value => ({ value, name: value })) } });
    window.__permissionRemote = request => request.operation === 'read' ? Promise.resolve(snapshot())
      : new Promise((resolve, reject) => state.pending.push({ request, reject, resolve: () => {
        state.preset = request.preset; state.revision++; resolve(snapshot());
      } }));
  });
}
export async function runPermissionCase(page, artifacts) {
  page.setDefaultTimeout(12000);
  const shell = page.locator('#settings-shell'), row = page.locator('#settings-permission');
  const trigger = row.getByRole('button');
  const open = async () => {
    await page.getByRole('button', { name: '更多设置', exact: true }).dispatchEvent('click');
    await page.getByRole('button', { name: '设置', exact: true }).dispatchEvent('click');
    await shell.getByRole('button', { name: '通用设置', exact: true }).click();
    await trigger.filter({ hasText: '工作区内修改' }).waitFor();
  };
  const select = async label => {
    await trigger.click();
    await page.getByRole('menuitem', { name: label, exact: true }).click();
  };
  const pending = () => page.waitForFunction(() => window.__permissionTest.pending.length > 0);
  const settle = failure => page.evaluate(failure => {
    const write = window.__permissionTest.pending.shift();
    if (failure) write.reject(new Error('fixture refused')); else write.resolve();
  }, failure);
  const dialog = page.getByRole('dialog', { name: '确认启用完全权限？', exact: true });
  await open();
  await page.screenshot({ path: join(artifacts, 'permission-default-row.png') });
  await select('完全权限'); await dialog.waitFor();
  assert.equal(await shell.getAttribute('aria-hidden'), 'true');
  const confirm = dialog.getByRole('button', { name: '启用完全权限', exact: true });
  assert.equal(await confirm.isDisabled(), true);
  assert.equal(await page.evaluate(() => window.__permissionTest.pending.length), 0);
  await dialog.getByRole('button', { name: '取消', exact: true }).click();
  await dialog.waitFor({ state: 'hidden' });
  assert.equal(await trigger.evaluate(node => node === document.activeElement), true);
  assert.equal(await trigger.innerText(), '工作区内修改');
  await select('完全权限'); await dialog.getByRole('checkbox').check();
  await page.screenshot({ path: join(artifacts, 'permission-default-confirm.png') });
  await confirm.click(); await pending();
  assert.equal(await trigger.isDisabled(), true);
  assert.equal(await trigger.innerText(), '工作区内修改');
  assert.equal(await page.evaluate(() => window.__permissionTest.pending[0].request.acknowledged), true);
  await settle(false); await trigger.filter({ hasText: '完全权限' }).waitFor();
  await select('仅可查看'); await pending(); await settle(true);
  await row.getByRole('alert').filter({ hasText: 'fixture refused' }).waitFor();
  assert.equal(await trigger.innerText(), '完全权限');
  await select('仅可查看'); await pending(); await settle(false);
  await trigger.filter({ hasText: '仅可查看' }).waitFor();
  await page.setViewportSize({ width: 620, height: 820 });
  await trigger.focus();
  await page.screenshot({ path: join(artifacts, 'permission-default-narrow.png') });
  await page.evaluate(() => { window.__permissionTest.preset = 'workspace-write'; window.__permissionTest.revision++;
    window.__verification.emit({ kind: 'permission_default_changed' }); });
  await trigger.filter({ hasText: '工作区内修改' }).waitFor();
  await select('完全权限');
  await page.keyboard.press('Escape'); await dialog.waitFor({ state: 'hidden' });
  assert.equal(await shell.isVisible(), true, 'nested Escape leaves settings open');
  await page.evaluate(() => { window.__permissionTest.readonly = true;
    window.__verification.emit({ kind: 'permission_default_changed' }); });
  await page.waitForFunction(() => document.querySelector('#settings-permission button').disabled);
  await shell.getByRole('button', { name: '关闭设置', exact: true }).click();
  await shell.waitFor({ state: 'hidden' });
  await writeFile(join(artifacts, 'permission-default-checks.json'), JSON.stringify({ passed: true,
    checks: ['official default row', 'full access acknowledgement', 'cancel and return focus', 'Host accepted value',
      'save failure and retry', 'external defaults change', 'narrow layout', 'nested Escape', 'read-only state'] }, null, 2));
}
