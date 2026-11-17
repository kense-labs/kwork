import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';

export async function installDeveloperMock(page) {
  await page.addInitScript(() => {
    const state = window.__developerTest = { enabled: true, pending: [] };
    window.__developerRemote = enabled => new Promise((resolve, reject) => state.pending.push({ enabled, reject,
      resolve: () => { state.enabled = enabled; resolve(enabled); } }));
  });
}
export async function runDeveloperCase(page, artifacts) {
  page.setDefaultTimeout(12000);
  const shell = page.locator('#settings-shell'), row = page.locator('#settings-developer');
  const toggle = row.getByRole('switch', { name: '显示代码工作视图', exact: true });
  const tabs = page.getByRole('tablist', { name: '会话视图', exact: true });
  const trajectory = tabs.getByRole('tab', { name: '轨迹', exact: true });
  const open = async () => {
    await page.getByRole('button', { name: '更多设置', exact: true }).dispatchEvent('click');
    await page.getByRole('button', { name: '设置', exact: true }).dispatchEvent('click');
    await shell.getByRole('button', { name: '通用设置', exact: true }).click();
    await toggle.waitFor(); await toggle.focus();
  };
  const close = async () => { await shell.getByRole('button', { name: '关闭设置', exact: true }).click(); await shell.waitFor({ state: 'hidden' }); };
  const pending = () => page.waitForFunction(() => window.__developerTest.pending.length > 0);
  const settle = failure => page.evaluate(failure => {
    const write = window.__developerTest.pending.shift();
    if (failure) write.reject(new Error('fixture refused')); else write.resolve();
  }, failure);
  await trajectory.waitFor(); await trajectory.dispatchEvent('click');
  await page.waitForFunction(() => [...document.querySelectorAll('[role=tab]')].some(node => node.getAttribute('aria-label') === '轨迹' && node.getAttribute('aria-selected') === 'true'));
  const preference = () => page.evaluate(() => Object.keys(localStorage).filter(key => key.startsWith('dsh.conversation.'))
    .map(key => JSON.parse(localStorage.getItem(key)).view));
  assert.ok((await preference()).includes('trajectory'));
  await open();
  assert.equal(await toggle.getAttribute('aria-checked'), 'true');
  await page.screenshot({ path: join(artifacts, 'coding-view-row.png') });
  await toggle.click(); await pending();
  assert.equal(await toggle.isDisabled(), true);
  assert.equal(await toggle.getAttribute('aria-checked'), 'true');
  await close(); await open();
  assert.equal(await toggle.getAttribute('aria-checked'), 'true');
  await toggle.click();
  await page.waitForFunction(() => document.querySelector('#settings-developer [role=switch]').disabled);
  assert.equal(await page.evaluate(() => window.__developerTest.pending.length), 1, 'reopened row shares the ordered write queue');
  await close(); await settle(false); await pending(); await settle(false);
  await tabs.waitFor({ state: 'hidden' });
  assert.ok((await preference()).includes('trajectory'), 'hiding coding views does not erase the session preference');
  await page.screenshot({ path: join(artifacts, 'coding-view-hidden.png') });
  await open(); assert.equal(await toggle.getAttribute('aria-checked'), 'false');
  await toggle.click(); await pending(); await settle(true);
  await row.getByRole('alert').filter({ hasText: '保存失败，请重试' }).waitFor();
  assert.equal(await toggle.getAttribute('aria-checked'), 'false');
  await toggle.click(); await pending(); await settle(false);
  await page.waitForFunction(() => document.querySelector('#settings-developer [role=switch]').getAttribute('aria-checked') === 'true');
  await close(); await trajectory.waitFor();
  assert.equal(await trajectory.getAttribute('aria-selected'), 'true', 're-enabling restores the session view');
  await tabs.getByRole('tab', { name: '对话', exact: true }).dispatchEvent('click');
  await page.waitForFunction(() => Object.keys(localStorage).some(key => key.startsWith('dsh.conversation.') && JSON.parse(localStorage.getItem(key)).view === 'chat'));
  await page.setViewportSize({ width: 620, height: 820 });
  await open(); await toggle.focus();
  await page.waitForFunction(() => {
    const control = document.querySelector('#settings-developer [role=switch]').getBoundingClientRect();
    const panel = document.querySelector('#settings-shell').getBoundingClientRect();
    return control.top >= panel.top + 54 && control.bottom <= panel.bottom;
  });
  await page.screenshot({ path: join(artifacts, 'coding-view-narrow.png') });
  await close();
  await writeFile(join(artifacts, 'coding-view-checks.json'), JSON.stringify({ passed: true,
    checks: ['official copy and switch', 'accepted state while saving', 'ordered writes across settings reopen', 'completion after settings close', 'coding tabs hidden',
      'session view preference retained', 'save failure and retry', 'preferred view restored', 'user-selected Chat retained', 'narrow focus scrolling'] }, null, 2));
}
