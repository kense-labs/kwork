import assert from 'node:assert/strict';
import { join } from 'node:path';
import { writeFile } from 'node:fs/promises';

export async function installModelsMock(page, fixture) {
  await page.addInitScript(fixture => {
    const state = { fixture, credentials: {}, writes: [], calls: [], conflict: false };
    window.__modelsTest = state;
    window.__modelsRemote = async ({ method, args }) => {
      state.calls.push({ method, args });
      const ok = value => ({ ok: true, value: structuredClone(value) });
      if (method === 'llm/listProviders') return ok(fixture.registered);
      if (method === 'llm/listConfigurableProviders') return ok(fixture.directory);
      if (method === 'settings/describe') return ok(fixture.described);
      if (method === 'session/modelCatalog') return ok({ groups: [] });
      if (method === 'credentials/describe') return ok(Object.fromEntries(args[0].map(ref => [ref,
        { configured: ref === 'DEEPSEEK_API_KEY' || !!state.credentials[ref], writable: true }])));
      if (method === 'credentials/set') { state.credentials[args[0]] = args[1]; return ok(null); }
      if (method === 'credentials/unset') { delete state.credentials[args[0]]; return ok(null); }
      if (method === 'llm/discoverModels') return new Promise(resolve => { state.discover = value => { delete state.discover; resolve(ok(value)); }; });
      if (method === 'settings/mutate') {
        const [ns, ops, revision] = args;
        const row = fixture.described.namespaces.find(row => row.ns === ns);
        if (state.conflict || revision != null && revision !== row.revision) {
          state.conflict = false;
          return { ok: false, error: { code: 'settings/conflict', message: 'changed' } };
        }
        state.writes.push(args);
        for (const op of ops) {
          for (const layer of ['user', 'value']) {
            let target = row[layer] ??= {};
            for (const part of op.path.slice(0, -1)) target = target[part] ??= {};
            if (op.op === 'unset') delete target[op.path.at(-1)]; else target[op.path.at(-1)] = structuredClone(op.value);
          }
          if (ns === 'llm-pi-ai' && op.path[0] === 'providers' && op.path.length === 2) {
            const id = op.path[1];
            if (op.op === 'unset') {
              fixture.directory = fixture.directory.filter(item => item.provider !== id);
              fixture.registered = fixture.registered.filter(item => item.id !== id);
            } else if (!fixture.directory.some(item => item.provider === id)) {
              fixture.directory.push({ provider: id, displayName: op.value.displayName || id, settingsNs: ns, settingsPath: op.path, declared: true });
              fixture.registered.push({ id, name: op.value.displayName || id });
            }
          }
        }
        row.revision++;
        return ok(row);
      }
      throw Error('Unknown fixture remote: ' + method);
    };
  }, fixture);
}

export async function runModelsCase(page, artifacts) {
  page.setDefaultTimeout(12000);
  const shell = page.locator('#settings-shell');
  const form = page.locator('#settings-models');
  const button = (name, parent = form) => parent.getByRole('button', { name, exact: true });
  await page.locator('#composer-submit-accessibility').getByRole('button', { name: '更多设置', exact: true }).dispatchEvent('click');
  await page.locator('#composer-submit-accessibility').getByRole('button', { name: '设置', exact: true }).dispatchEvent('click');
  const navigation = shell.getByRole('navigation');
  await navigation.waitFor();
  assert.equal(await navigation.getByRole('button', { name: 'API Key', exact: true }).count(), 0);
  assert.equal(await navigation.getByRole('button', { name: '模型服务', exact: true }).count(), 0);
  assert.equal(await navigation.getByRole('button', { name: '模型', exact: true }).count(), 1);
  await navigation.getByRole('button', { name: '模型', exact: true }).click();
  await button('添加模型提供商').waitFor();
  assert.equal(await form.getByRole('heading', { name: '模型', exact: true }).count(), 1);
  await page.screenshot({ path: join(artifacts, 'models-list.png') });
  await button('添加模型提供商').click();
  const catalog = form.getByRole('tab', { name: '第三方模型提供商', exact: true });
  const custom = form.getByRole('tab', { name: '自定义模型 API', exact: true });
  const key = form.getByRole('textbox', { name: 'API 密钥', exact: true });
  const keyInput = form.locator('input[aria-label="API 密钥"]:visible');
  await keyInput.fill('catalog-draft');
  await custom.click();
  const route = form.getByRole('textbox', { name: 'Provider ID', exact: true });
  await route.fill('parity-gateway');
  await form.getByRole('textbox', { name: '显示名称', exact: true }).fill('Parity Gateway');
  await form.getByRole('textbox', { name: 'API 地址', exact: true }).fill('https://fixture.invalid/v1');
  await keyInput.fill('synthetic-key');
  await catalog.click();
  assert.equal(await keyInput.inputValue(), 'catalog-draft');
  await custom.click();
  assert.equal(await route.inputValue(), 'parity-gateway');
  assert.equal(await keyInput.inputValue(), 'synthetic-key');
  const foreground = await button('创建提供商').evaluate(node => getComputedStyle(node).color);
  const background = await button('创建提供商').evaluate(node => getComputedStyle(node).backgroundColor);
  assert.notEqual(foreground, background, 'primary action retains its official foreground color');
  assert.ok(await button('创建提供商').evaluate(node => {
    const rgb = getComputedStyle(node).color.match(/\d+/g).slice(0, 3).map(Number);
    return rgb.every(channel => channel < 100);
  }), 'dark theme primary action has dark foreground on its light fill');
  await page.screenshot({ path: join(artifacts, 'models-custom-draft.png') });
  await button('获取可用模型').click();
  await page.waitForFunction(() => !!window.__modelsTest.discover);
  await page.waitForFunction(() => [...document.querySelectorAll('#settings-models [role=tab]')].every(tab => tab.disabled));
  assert.equal(await catalog.isDisabled(), true, 'mode stays put during discovery');
  await page.evaluate(() => window.__modelsTest.discover([{ id: 'chat-1', name: 'Chat One', contextWindow: 32000, maxTokens: 8192, input: ['text'] }, { id: 'chat-2', name: 'Chat Two' }]));
  const picker = page.getByRole('dialog', { name: '选择模型', exact: true });
  await picker.waitFor();
  assert.equal(await shell.getAttribute('inert'), '', 'nested picker owns the background');
  await page.screenshot({ path: join(artifacts, 'models-discovery.png') });
  await picker.getByRole('checkbox').first().check();
  await button('更新列表', picker).click();
  await picker.waitFor({ state: 'hidden' });
  assert.equal(await form.getByRole('textbox', { name: '模型 ID 1', exact: true }).inputValue(), 'chat-1');
  await button('创建提供商').click();
  const edit = button('编辑 Parity Gateway (parity-gateway)');
  await edit.waitFor();
  await edit.click();
  await form.locator('summary').filter({ hasText: '自定义设置' }).click();
  const endpoint = form.getByRole('textbox', { name: 'API 地址', exact: true });
  await endpoint.fill('https://edited.fixture.invalid/v1');
  await page.evaluate(() => { window.__modelsTest.conflict = true; });
  await button('保存').click();
  await form.getByText('这张卡片打开期间，这些设置已被其他地方改动。请关闭后重新打开，在当前值上编辑。', { exact: true }).waitFor();
  assert.equal(await endpoint.inputValue(), 'https://edited.fixture.invalid/v1');
  await page.screenshot({ path: join(artifacts, 'models-conflict.png') });
  await button('取消').click();
  await button('删除 Parity Gateway (parity-gateway)').click();
  const deletion = page.getByRole('dialog', { name: '删除 Parity Gateway (parity-gateway)？', exact: true });
  await deletion.waitFor();
  await page.keyboard.press('Escape');
  await deletion.waitFor({ state: 'hidden' });
  assert.ok(await button('删除 Parity Gateway (parity-gateway)').evaluate(node => document.activeElement === node));
  await button('删除 Parity Gateway (parity-gateway)').click();
  await deletion.getByRole('button', { name: /删除 Parity Gateway/ }).click();
  await deletion.waitFor({ state: 'hidden' });
  await edit.waitFor({ state: 'hidden' });
  assert.equal(await page.evaluate(() => window.__modelsTest.credentials.PARITY_GATEWAY_API_KEY), undefined);
  await page.setViewportSize({ width: 620, height: 820 });
  await button('添加模型提供商').click();
  await custom.click();
  await page.screenshot({ path: join(artifacts, 'models-narrow.png') });
  const overflow = await form.evaluate(node => node.scrollWidth > node.clientWidth + 1);
  assert.equal(overflow, false);
  const last = button('取消'); await last.focus();
  assert.ok(await last.evaluate(node => { const r = node.getBoundingClientRect(), p = node.closest('#settings-models').getBoundingClientRect(); return r.top >= p.top && r.bottom <= p.bottom; }));
  await page.keyboard.press('Escape');
  await shell.waitFor({ state: 'hidden' });
  await writeFile(join(artifacts, 'models-checks.json'), JSON.stringify({ passed: true, checks: ['provider cards', 'independent drafts', 'discovery busy gate', 'nested modal ownership', 'credential create/delete', 'revision conflict preserves draft', 'narrow scroll and focus', 'Escape restores workspace'] }, null, 2));
}
