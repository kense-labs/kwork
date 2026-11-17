import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';

export async function runFontsCase(page, artifacts) {
  page.setDefaultTimeout(12000);
  const shell = page.locator('#settings-shell'), theme = page.locator('#settings-theme');
  const more = theme.getByRole('button', { name: '更多字体设置', exact: true });
  const open = async () => {
    await page.getByRole('button', { name: '更多设置', exact: true }).dispatchEvent('click');
    await page.getByRole('button', { name: '设置', exact: true }).dispatchEvent('click');
    await shell.getByRole('button', { name: '通用设置', exact: true }).click();
    await more.waitFor();
  };
  const close = async () => { await shell.getByRole('button', { name: '关闭设置', exact: true }).click(); await shell.waitFor({ state: 'hidden' }); };
  const saved = (field, value) => page.waitForFunction(({field, value}) =>
    window.__verification.calls.filter(call => call.name === 'client_preferences' && call.args.request.operation === 'set').at(-1)?.args.request.changes[field] === value,
    {field:field === 'contentFontDelta' ? 'fontSize' : field, value:field === 'contentFontDelta' ? value+14 : value});
  const step = async (label, field, value) => { const button = theme.getByRole('button', { name: label, exact: true }); await button.focus(); await button.press('Enter'); await saved(field, value); await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))); };
  await open(); assert.equal(await more.getAttribute('aria-expanded'), 'false');
  await more.click();
  const fields = ['正文字体', '代码字体', '终端字体'].map(name => theme.getByRole('textbox', {name, exact: true}));
  for (const field of fields) await field.waitFor();
  const names = ['textFontFamily', 'codeFontFamily', 'terminalFontFamily'];
  const authored = ['  "Missing Font", Courier   New , serif  ', 'Courier New, monospace', 'Menlo'];
  const normalized = ['"Missing Font", "Courier New", serif', '"Courier New", monospace', '"Menlo"'];
  for (let index = 0; index < fields.length; index++) {
    await fields[index].focus(); await fields[index].fill(authored[index]);
    await fields[index].press('Enter'); await saved(names[index], normalized[index]);
    assert.equal(await fields[index].inputValue(), normalized[index]);
  }
  await page.waitForFunction(() => getComputedStyle(document.querySelector('#settings-theme input[aria-label="正文字体"]')).fontFamily.includes('Courier New'));
  // Composition Enter must not commit an unfinished CJK font name.
  await fields[0].focus(); await fields[0].fill('未完成');
  await fields[0].dispatchEvent('keydown', { key: 'Enter', keyCode: 229, isComposing: true });
  assert.equal(await fields[0].evaluate(node => node === document.activeElement), true);
  await fields[0].fill(normalized[0]); await fields[0].press('Enter');
  for (let value = 12; value <= 16; value++) await step('增大代码字号', 'codeFontSize', value);
  assert.equal(await theme.getByRole('button', {name:'增大代码字号',exact:true}).isDisabled(), true);
  for (let value = 14; value <= 20; value++) await step('增大终端字号', 'terminalFontSize', value);
  assert.equal(await theme.getByRole('button', {name:'增大终端字号',exact:true}).isDisabled(), true);
  await step('增大字号', 'contentFontDelta', 1);
  await page.screenshot({timeout:30000, path: join(artifacts, 'fonts-expanded-wide.png') });
  await close();
  await page.getByRole('tab', { name: '轨迹', exact: true }).dispatchEvent('click');
  await page.locator('[data-trajectory-accessibility="controls"]').getByRole('button', { name: /^#\d+ 工具 run_code Inspect structured source/ }).dispatchEvent('click');
  await page.getByRole('tablist', { name: '事件详情', exact: true }).getByRole('tab', {name:'源码',exact:true}).dispatchEvent('click');
  const code = page.locator('.trajectory-program-full pre.shiki'); await code.waitFor();
  assert.equal(await code.evaluate(node => getComputedStyle(node).fontSize), '12px');
  assert.match(await code.evaluate(node => getComputedStyle(node).fontFamily), /Courier New/);
  await page.screenshot({timeout:30000,path:join(artifacts,'code-role-applied.png')});
  await open(); assert.equal(await more.getAttribute('aria-expanded'), 'false', 'expanded region resets on every mount');
  await step('减小字号','contentFontDelta',0); await close();
  assert.equal(await code.evaluate(node => getComputedStyle(node).fontSize), '12px', 'official trajectory source keeps its fixed 12px detail typography');
  await open(); await more.click();
  const reads = await page.evaluate(() => window.__verification.calls.filter(call => call.name === 'client_preferences' && call.args.request.operation === 'read').length);
  await page.evaluate(() => { window.__fontSaveFailure = 1; });
  const decreaseCode = theme.getByRole('button', {name:'减小代码字号',exact:true});
  await decreaseCode.focus(); await decreaseCode.press('Enter');
  await page.waitForFunction(reads => window.__verification.calls.filter(call => call.name === 'client_preferences' && call.args.request.operation === 'read').length > reads, reads);
  await page.waitForFunction(() => document.body.style.getPropertyValue('--dsh-code-font-size') === '16px');
  await close();
  // Real terminal renderer/engine must refit when its independent font size changes.
  await page.evaluate(() => window.__verification.emit({kind:'agent_terminals',sessionId:'session-markdown-tools-browser',terminals:[{
    id:'font-terminal',title:'Font grid',cwd:'/fixture',cols:100,rows:30,state:'running',exitCode:null,origin:'agent',control:'agent',sandboxMode:'workspace-write',shell:'zsh'
  }]}));
  const grid = () => page.evaluate(() => window.__verification.calls.filter(call => call.name === 'agent_terminal_input' && call.args.input?.type === 'resize').at(-1)?.args.input);
  await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'agent_terminal_input' && call.args.input?.type === 'resize'));
  const large = await grid();
  await open(); await more.click();
  for (let value = 19; value >= 10; value--) await step('减小终端字号','terminalFontSize',value);
  assert.equal(await theme.getByRole('button', {name:'减小终端字号',exact:true}).isDisabled(), true);
  await close();
  await page.waitForFunction(large => {
    const small=window.__verification.calls.filter(call=>call.name==='agent_terminal_input' && call.args.input?.type==='resize').at(-1)?.args.input;
    return small?.cols > large.cols * 1.5 && small?.rows > large.rows * 1.5;
  },large);
  const small = await grid();
  await page.screenshot({timeout:30000,path:join(artifacts,'terminal-refit.png')});
  await page.getByRole('tab', { name: '轨迹', exact: true }).dispatchEvent('click');
  await page.keyboard.press('Meta+Equal');
  await saved('terminalFontSize',11); await saved('contentFontDelta',1);
  assert.equal(await page.evaluate(() => document.body.style.getPropertyValue('--dsh-code-font-size')), '16px', 'global shortcut clamps each role independently');
  await page.reload();
  await page.waitForFunction(() => document.querySelector('canvas') && !document.getElementById('boot'));
  await open(); await more.click();
  assert.equal(await fields[1].inputValue(), normalized[1]);
  assert.equal(await page.evaluate(() => document.body.style.getPropertyValue('--dsh-code-font-size')), '16px');
  assert.equal(await page.evaluate(() => document.body.style.getPropertyValue('--dsh-terminal-font-size')), '11px');
  await page.setViewportSize({width:620,height:820});
  await fields[2].focus();
  await page.waitForFunction(() => {
    const control=document.querySelector('#settings-theme input[aria-label="终端字体"]').getBoundingClientRect();
    const panel=document.querySelector('#settings-shell').getBoundingClientRect();
    return control.top>=panel.top+54 && control.bottom<=panel.bottom;
  });
  await page.screenshot({timeout:30000,path:join(artifacts,'fonts-expanded-narrow.png')});
  await fields[2].fill(''); await fields[2].press('Enter'); await saved('terminalFontFamily','');
  await page.waitForFunction(() => document.body.style.getPropertyValue('--dsh-font-family-terminal') === '');
  assert.equal(await page.evaluate(() => document.body.style.getPropertyValue('--dsh-font-family-terminal')), '');
  await close();
  await page.evaluate(() => {
    const theme=window.__clientPreferenceState.namespaces.find(row=>row.ns==='ui-theme');
    theme.value.fontSize=18;theme.revision++;
    window.__verification.emit({kind:'client_preferences_changed'});
  });
  await page.waitForFunction(() => document.body.style.getPropertyValue('--dsh-content-font-size')==='18px');
  await writeFile(join(artifacts,'font-checks.json'),JSON.stringify({passed:true,large,small,checks:[
    'official expansion and reset', 'three role families and normalization', 'IME Enter', 'independent role limits',
    'program source uses the configured family and official fixed detail size', 'failed save recovers durable font values', 'real terminal renderer grid refit', 'global shortcut adjusts all roles with separate limits', 'cold browser persistence', 'narrow keyboard reveal', 'empty family restores default', 'Host-only field writes', 'external Host change applies while settings are closed'
  ]},null,2));
}
