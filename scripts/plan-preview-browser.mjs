import assert from 'node:assert/strict';
import { join } from 'node:path';

const code = 'const sample = "' + 'long source '.repeat(16) + '";';
export const planFixture = [
  '# 官方计划预览',
  '| 步骤 | 检查 |\n| :--- | :--- |\n| 1 | $E=mc^2$ |',
  '来源[^note]。\n\n[^note]: 计划脚注。',
  '```javascript\n' + code + '\n```',
  ...Array.from({length: 32}, (_, i) => `## 计划步骤 ${i + 1}\n\n验证窄栏折行和滚动位置。`),
  '计划结束标记。',
].join('\n\n');

export async function runPlanPreviewCase(page, artifacts, sessionId) {
  const view = page.locator('section[data-plan-preview]');
  await page.getByRole('button', {name:'打开计划：官方计划预览',exact:true}).click();
  await view.waitFor();
  assert.equal(await view.getAttribute('data-plan-preview'), 'fixture-plan');
  assert.equal(await view.locator('table').count(), 1);
  await view.locator('.katex').waitFor();
  await view.getByText('计划脚注。').waitFor();
  await page.screenshot({ path: join(artifacts, 'plan-wide.png') });

  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  await view.getByRole('button', { name: '复制', exact: true }).click();
  assert.equal(await page.evaluate(() => navigator.clipboard.readText()), code);
  const wrap = view.getByRole('button', { name: '自动换行', exact: true });
  const wrapped = await wrap.getAttribute('aria-pressed');
  await wrap.click();
  assert.notEqual(await wrap.getAttribute('aria-pressed'), wrapped);

  await page.setViewportSize({ width: 740, height: 650 });
  await page.waitForFunction(() => getComputedStyle(document.querySelector('[data-plan-preview]')).paddingLeft === '18px');
  await view.getByText('计划结束标记。').scrollIntoViewIfNeeded();
  assert.ok(await view.evaluate(node => node.scrollTop > 0));
  await page.screenshot({ path: join(artifacts, 'plan-narrow-end.png') });

  // Reload a persisted plan tab: show a real pending state, then the mapped
  // not-found diagnostic. The regular chat history alone cannot satisfy this read.
  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.reload();
  await page.getByRole('status').filter({hasText: '正在读取计划…'}).waitFor();
  await page.waitForFunction(() => !!window.__verification.settlePlan);
  await page.evaluate(() => window.__verification.settlePlan({}));
  await page.getByRole('status').filter({hasText: '未找到这份计划'}).waitFor();
  await page.screenshot({ path: join(artifacts, 'plan-read-error.png') });

  // An in-memory review has the official temporary resource identity and
  // owns a different document without modifying the persisted history tab.
  await page.evaluate(sessionId => window.__verification.emit({kind: 'session_update', sessionId,
    update: {sessionUpdate: 'gpui_plan_review', phase: 'open', requestId: 'temporary', question: '批准计划？',
      markdown: '# 临时计划\n\n**真实审核内容**'} }), sessionId);
  await view.getByRole('heading', {name: '临时计划'}).waitFor();
  assert.match(await view.getAttribute('data-plan-preview'), /^dsh-resource:\/\/plan-review\//);
  assert.equal(await view.locator('strong').textContent(), '真实审核内容');
  await page.screenshot({ path: join(artifacts, 'plan-temporary.png') });
  const review = page.locator('[data-plan-review-key="temporary"]');
  await review.getByRole('heading', {name:'临时计划'}).waitFor();
  const approve = review.getByRole('button',{name:'同意执行',exact:true});
  await approve.click();
  await review.getByRole('status').filter({hasText:'验收夹具拒绝首次提交'}).waitFor();
  await page.screenshot({path:join(artifacts,'plan-review-error.png')});
  await approve.click();
  await review.waitFor({state:'detached'});
}
