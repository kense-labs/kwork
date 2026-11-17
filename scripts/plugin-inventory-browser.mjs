import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';

const entry = (id, enabled = true, fiberPhase = 'active') => ({ entryId: id,
  moduleName: '@fixture/' + id, enabled, fiberPhase,
  meta: { title: { zh: id, en: id }, description: { zh: '插件的详细说明。', en: 'Inventory fixture.' } } });
const fixture = { entries: [entry('normal'), { ...entry('failed', true, 'failed'), diagnostic: 'Fixture dependency failed' },
  entry('pending', true, 'pending'), entry('disabled', false, null), entry('shared', false, null)],
  agentPresets: [
    { id: 'standard', isDefault: true, rows: [entry('shared'), { ...entry('conditional', 'conditional', null), condition: 'platform == windows' }] },
    { id: 'secondary', name: '第二预设', isDefault: false, rows: [entry('SecondaryOnly')] },
  ] };

export async function installInventoryMock(page) {
  await page.addInitScript(() => {
    window.__inventoryTest = { pending: [], calls: 0 };
    window.__inventoryRemote = () => {
      window.__inventoryTest.calls++;
      return new Promise((resolve, reject) => window.__inventoryTest.pending.push({ resolve, reject }));
    };
  });
}

export async function runInventoryCase(page, artifacts) {
  page.setDefaultTimeout(12000);
  const shell = page.locator('#settings-shell');
  const inventory = page.locator('#settings-plugins');
  const navigation = shell.getByRole('navigation');
  const nav = navigation.getByRole('button', { name: '内置插件', exact: true });
  const close = shell.getByRole('button', { name: '关闭设置', exact: true });
  const search = inventory.getByRole('searchbox', { name: '搜索插件', exact: true });
  const group = inventory.getByRole('button', { name: '全局插件', exact: true });
  const settle = async (snapshot = fixture, failure = false) => {
    await page.waitForFunction(() => window.__inventoryTest.pending.length > 0);
    await page.evaluate(({ snapshot, failure }) => {
      const request = window.__inventoryTest.pending.shift();
      request[failure ? 'reject' : 'resolve'](snapshot);
    }, { snapshot, failure });
  };
  const open = async () => {
    await page.locator('#composer-submit-accessibility').getByRole('button', { name: '更多设置', exact: true }).dispatchEvent('click');
    await page.locator('#composer-submit-accessibility').getByRole('button', { name: '设置', exact: true }).dispatchEvent('click');
    await nav.click();
  };
  await open();
  await inventory.getByRole('status').filter({ hasText: '正在读取插件' }).waitFor();
  await settle('Fixture inventory failure', true);
  await inventory.getByRole('alert').filter({ hasText: '暂时无法读取插件。' }).waitFor();
  await inventory.getByRole('button', { name: '重试', exact: true }).click();
  await settle();
  await search.waitFor();
  assert.equal(await inventory.getByRole('heading', { name: '内置插件', exact: true }).count(), 1);
  assert.equal(await inventory.getByRole('tablist').count(), 0, 'one official inventory contribution needs no artificial tab bar');
  assert.equal(await group.getAttribute('aria-expanded'), 'false');
  assert.equal(await inventory.getByRole('button', { name: '会话插件', exact: true }).getAttribute('aria-expanded'), 'true');
  await group.click();
  const global = inventory.locator('[data-plugin-scope="global"]');
  await global.locator('[data-plugin-entry="failed"]').waitFor();
  assert.equal(await global.locator('[data-plugin-entry]').first().getAttribute('data-plugin-entry'), 'failed');
  await global.locator('[data-plugin-entry="failed"] > button').click();
  assert.ok((await global.locator('[data-plugin-entry="failed"]').innerText()).includes('运行状态'));
  assert.ok((await global.locator('[data-plugin-entry="failed"]').innerText()).includes('启动失败'));
  assert.ok((await global.locator('[data-plugin-entry="shared"]').innerText()).includes('预设中启用'));
  await close.focus();
  await page.keyboard.press('Tab');
  assert.equal(await search.evaluate(node => node === document.activeElement), true, 'header Tab enters the official inventory');
  await search.fill('SecondaryOnly');
  await inventory.getByRole('button', { name: '第二预设', exact: true }).click();
  await inventory.locator('[data-preset-id="secondary"] [data-plugin-entry="SecondaryOnly"]').waitFor();
  await search.fill('');
  const picker = inventory.getByRole('button', { name: '选择要查看的 Agent 预设', exact: true });
  await picker.click();
  await page.getByRole('menuitem').filter({ hasText: '默认' }).click();
  await inventory.locator('[data-preset-id="standard"]').waitFor();
  await search.fill('NothingCouldMatchThis123');
  await inventory.getByText('没有匹配的插件。', { exact: true }).waitFor();
  await search.fill('');
  await global.locator('[data-plugin-entry="normal"]').waitFor();
  assert.equal(await global.locator('ul').evaluate(node => getComputedStyle(node).gridTemplateColumns.split(' ').length), 2);
  await page.screenshot({ path: join(artifacts, 'inventory-wide.png') });
  await page.setViewportSize({ width: 620, height: 820 });
  await page.waitForFunction(() => document.querySelector('#settings-plugins')?.getBoundingClientRect().width < 520);
  assert.equal(await global.locator('ul').evaluate(node => getComputedStyle(node).gridTemplateColumns.split(' ').length), 1);
  await global.locator('[data-plugin-entry="disabled"] > button').focus();
  assert.equal(await inventory.evaluate(node => node.scrollTop > 0), true, 'native focus scrolls the inventory into view');
  await page.screenshot({ path: join(artifacts, 'inventory-narrow.png') });
  await close.click();
  await open();
  await inventory.getByRole('status').filter({ hasText: '正在读取插件' }).waitFor();
  await close.click();
  await open();
  await settle({ entries: [entry('obsolete')] });
  await settle();
  await search.waitFor();
  await group.click();
  assert.equal(await inventory.locator('[data-plugin-entry="obsolete"]').count(), 0, 'a closed page cannot publish a late inventory');
  await search.focus();
  await page.keyboard.press('Escape');
  await shell.waitFor({ state: 'hidden' });
  await writeFile(join(artifacts, 'inventory-checks.json'), JSON.stringify({ passed: true,
    checks: ['loading and retry', 'official cards and groups', 'Host failure state', 'preset search and switcher',
      'read-only state tags', 'two/one-column layout', 'keyboard focus and scrolling', 'closed request ignored', 'Escape cleanup'] }, null, 2));
}
