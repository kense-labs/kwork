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

  await page.evaluate(sessionId => window.__verification.emit({kind:'session_update', sessionId,
    update:{sessionUpdate:'gpui_questions', phase:'open', requestId:'question-layout', questions:[
      {id:'layout-1', question:'这篇读后感是围绕哪部作品来写？', options:[
        {label:'工作区已有的文章（推荐）', description:'根据工作区现有的文章写一篇约一千字的读后感。'},
        {label:'另一本书或另一篇文章，题目和内容由我在下一步指定', description:'请提供作品的标题、作者和适合在读后感中讨论的主要内容。'},
      ]},
      {id:'layout-2', question:'希望使用什么风格？', options:[{label:'自然清晰'}]},
    ]}}), sessionId);
  const layout = page.locator('[data-question-key="question-layout"]');
  await layout.waitFor();
  assert.equal(await layout.evaluate(node => [...node.querySelectorAll('[data-option-line]')].every(line =>
    line.nextElementSibling.getBoundingClientRect().top >= line.getBoundingClientRect().bottom)), true,
  'descriptions consistently sit below the option titles');
  await page.screenshot({path:join(artifacts,'question-options-wide.png')});
  await page.setViewportSize({width:365, height:520});
  await page.waitForFunction(() => document.querySelector('[data-question-key="question-layout"]')
    ?.closest('[data-chat-markdown]')?.style.clipPath);
  // The fullscreen dock's rightmost toolbar icon returns to the conversation.
  await page.mouse.click(345, 24);
  await page.waitForFunction(() => {
    const frame = document.querySelector('[data-question-key="question-layout"]')?.closest('[data-chat-markdown]');
    return frame && !frame.style.clipPath;
  });
  const next = layout.getByText('下一题', {exact:true});
  await next.waitFor();
  const nextBox = await next.boundingBox();
  assert.ok(nextBox.x >= 0 && nextBox.x + nextBox.width <= 365 && nextBox.y + nextBox.height <= 520,
    'the primary action stays inside a narrow window');
  await page.screenshot({path:join(artifacts,'question-options-narrow.png')});
  await layout.getByRole('radio',{name:/另一本书/}).click();
  await layout.getByText('希望使用什么风格？', {exact:true}).waitFor();
}
