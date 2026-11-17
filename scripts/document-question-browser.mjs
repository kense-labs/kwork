import assert from 'node:assert/strict';
import { join } from 'node:path';

export async function runDocumentAndQuestionCase(page, artifacts, sessionId) {
  const recorded = page.locator('[data-question-reply]');
  await recorded.getByText('历史提问：保留哪些内容？', { exact: true }).waitFor();
  await recorded.getByText('保留完整的历史回答。', { exact: true }).waitFor();
  await page.screenshot({ path: join(artifacts, 'question-reply.png') });
  await page.locator('[data-chat-markdown]').getByRole('button', {name:'打开文档', exact:true}).click();
  const document = page.locator('[data-document-markdown]');
  await document.getByRole('heading', {name:'分页文档'}).waitFor();
  const metadata = document.locator('[data-document-frontmatter]');
  assert.equal(await metadata.locator('dt').first().textContent(), '标题');
  assert.equal(await metadata.locator('dd').last().textContent(), '1.0');
  await page.getByRole('button', {name:'加载更多', exact:true}).click();
  const retry = page.locator('[data-dock-preview=document]').getByRole('button', {name:'重试',exact:true});
  await retry.waitFor();
  await document.getByRole('heading', {name:'分页文档'}).waitFor();
  await page.screenshot({path:join(artifacts,'document-load-error.png')});
  await retry.click();
  await document.getByText(/文档最后一段/).waitFor();
  await document.locator('.katex').waitFor();
  await document.getByAltText('文件旁图片').waitFor();
  await page.waitForFunction(() => document.querySelector('[data-document-markdown] img')?.naturalWidth === 64);
  assert.ok(await page.evaluate(() => window.__verification.calls.some(call =>
    call.name === 'document_file' && call.args.path.endsWith('/docs/preview image.png'))));
  await page.screenshot({path:join(artifacts,'document-paged.png')});

  await page.evaluate(sessionId => window.__verification.emit({kind:'session_update',sessionId,
    update:{sessionUpdate:'gpui_questions',phase:'open',requestId:'question-fixture',questions:[
      {id:'question-1',question:'如何处理？', detail:'## 提问详情\n\n| 操作 | 说明 |\n| --- | --- |\n| 保留 | **继续检查** |',
        options:[{label:'继续（推荐）'},{label:'停止'}]},
    ]}}), sessionId);
  const detail = page.locator('[data-question-key="question-fixture"]');
  await detail.waitFor();
  await detail.getByRole('heading',{name:'提问详情'}).waitFor();
  await detail.getByRole('table').waitFor();
  await detail.getByRole('radio',{name:/继续/}).waitFor();
  await page.screenshot({path:join(artifacts,'question-detail.png')});
  const submit = detail.getByRole('button',{name:'提交',exact:true});
  await submit.click();
  await detail.getByText(/验收夹具拒绝首次提交/).waitFor();
  await page.screenshot({path:join(artifacts,'question-answer-error.png')});
  await submit.click();
  await detail.waitFor({state:'detached'});
  const answers = await page.evaluate(() => window.__verification.calls.filter(call => call.name === 'respond_question'));
  assert.equal(answers.length, 2);
  for (const answer of answers) assert.deepEqual(answer.args.answers, [{id:'question-1', selected:['继续（推荐）']}]);
}
