import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';

export async function installAgentPresetsMock(page) {
  await page.addInitScript(() => {
    const state = window.__presetTest = { selected: 'standard', revision: 1, calls: [],
      failSave: false, delayedRead: false, reads: [], pendingSave: null };
    const rows = () => ['standard', 'ptc', 'minimal', 'cordis', 'custom', 'broken'].map(id => ({ id,
      isDefault: id === state.selected,
      ...(id === 'custom' ? { name: '团队模式', description: '团队自定义工具组合。' } : {}),
      ...(id === 'broken' ? { name: '故障预设', broken: 'Fixture missing dependency' } : {}) }));
    window.__presetRemote = async request => {
      state.calls.push(request);
      if (request.operation === 'list') return { presets: rows(), writable: true, revision: state.revision };
      if (request.operation === 'read') {
        const result = { content: `- id: ${request.id}\n  name: fixture\n`, name: request.id };
        if (state.delayedRead) return new Promise(resolve => state.reads.push(() => resolve(result)));
        return result;
      }
      if (request.operation === 'default') {
        await new Promise(resolve => { state.pendingSave = resolve; });
        state.pendingSave = null;
        if (state.failSave) { state.failSave = false; throw Error('Fixture write conflict'); }
        if (request.revision !== state.revision) throw Error('Fixture stale revision');
        state.selected = request.id; state.revision++;
        return { presets: rows(), writable: true, revision: state.revision };
      }
      throw Error('Unexpected preset request');
    };
  });
}

export async function runAgentPresetsCase(page, artifacts) {
  page.setDefaultTimeout(12000);
  const shell = page.locator('#settings-shell'), content = page.locator('#settings-agent-presets');
  const nav = shell.getByRole('navigation').getByRole('button', { name: 'Agent 预设', exact: true });
  const close = shell.getByRole('button', { name: '关闭设置', exact: true });
  const card = id => content.locator(`[data-agent-preset-id="${id}"]`);
  const selection = id => card(id).locator('button[aria-pressed]');
  const open = async () => {
    await page.locator('#composer-submit-accessibility').getByRole('button', { name: '更多设置', exact: true }).dispatchEvent('click');
    await page.locator('#composer-submit-accessibility').getByRole('button', { name: '设置', exact: true }).dispatchEvent('click');
    await nav.click(); await card('standard').waitFor();
  };
  const settleSave = async () => {
    await page.waitForFunction(() => window.__presetTest.pendingSave !== null);
    await page.evaluate(() => window.__presetTest.pendingSave());
  };
  await open();
  assert.equal(await content.getByRole('heading', { name: 'Agent 预设', exact: true }).count(), 1);
  assert.equal(await content.locator('[data-agent-preset-id]').count(), 6);
  assert.equal(await selection('standard').getAttribute('aria-pressed'), 'true');
  assert.equal(await selection('broken').isDisabled(), true);
  await selection('broken').focus(); await page.keyboard.press('Enter');
  assert.equal(await page.evaluate(() => window.__presetTest.calls.filter(c => c.operation === 'default').length), 0);
  await page.evaluate(() => { window.__presetTest.failSave = true; });
  await selection('minimal').click();
  assert.equal(await selection('ptc').isDisabled(), true);
  assert.equal(await selection('standard').getAttribute('aria-pressed'), 'true');
  await settleSave();
  await content.getByRole('alert').filter({ hasText: 'Fixture write conflict' }).waitFor();
  assert.equal(await selection('standard').getAttribute('aria-pressed'), 'true');
  await selection('minimal').click(); await settleSave();
  await page.waitForFunction(() => document.querySelector('[data-agent-preset-id="minimal"] button[aria-pressed]')?.getAttribute('aria-pressed') === 'true');
  const viewer = card('standard').getByRole('button', { name: '查看配置: 标准模式', exact: true });
  await viewer.click();
  const yaml = page.getByRole('dialog', { name: '查看配置 · 标准模式', exact: true });
  await yaml.waitFor(); assert.ok((await yaml.innerText()).includes('- id: standard'));
  assert.equal(await yaml.locator('textarea,input,[contenteditable=true]').count(), 0);
  await page.keyboard.press('Escape'); await yaml.waitFor({ state: 'hidden' });
  assert.equal(await shell.isVisible(), true);
  assert.equal(await viewer.evaluate(node => node === document.activeElement), true);
  await card('standard').getByRole('button', { name: '模式说明: 标准模式', exact: true }).click();
  const guide = page.getByRole('dialog', { name: '标准模式', exact: true });
  await guide.waitFor();
  const usage = guide.getByRole('tab', { name: '如何使用', exact: true });
  await usage.click(); assert.equal(await usage.getAttribute('aria-selected'), 'true');
  await guide.getByRole('tabpanel').filter({ visible: true }).waitFor();
  await page.screenshot({ path: join(artifacts, 'preset-guide.png') });
  await usage.focus(); await page.keyboard.press('Escape');
  await guide.waitFor({ state: 'hidden' }); assert.equal(await shell.isVisible(), true);
  await page.screenshot({ path: join(artifacts, 'presets-wide.png') });
  assert.equal(await card('standard').locator('..').evaluate(node => getComputedStyle(node).gridTemplateColumns.split(' ').length), 2);
  await page.setViewportSize({ width: 620, height: 820 });
  await page.waitForFunction(() => document.querySelector('#settings-agent-presets')?.getBoundingClientRect().width < 530);
  assert.equal(await card('standard').locator('..').evaluate(node => getComputedStyle(node).gridTemplateColumns.split(' ').length), 1);
  await page.screenshot({ path: join(artifacts, 'presets-narrow.png') });
  await page.evaluate(() => { window.__presetTest.delayedRead = true; });
  await viewer.click();
  await page.waitForFunction(() => window.__presetTest.reads.length === 1);
  await close.click(); await open();
  await page.evaluate(() => window.__presetTest.reads.shift()());
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  assert.equal(await yaml.isVisible(), false, 'closed connection owner cannot reopen a stale YAML viewer');
  await nav.focus(); await page.keyboard.press('Escape'); await shell.waitFor({ state: 'hidden' });
  await writeFile(join(artifacts, 'agent-presets-checks.json'), JSON.stringify({ passed: true,
    checks: ['official built-in/custom cards', 'broken composition cannot select', 'pending save retains accepted default',
      'save failure/retry', 'read-only YAML', 'nested Escape and focus return', 'Markdown usage guide',
      'two/one-column layouts', 'late viewer after close ignored'] }, null, 2));
}
