import assert from 'node:assert/strict';
import { join } from 'node:path';
import { readFile, writeFile } from 'node:fs/promises';

const nativeCsp = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8')).app.security.csp;

export const markdownFixture = [
  '## 官方正文排版',
  '点击 [完整预览](完整预览.html)，直接改它就能重新排盘。' + '中文与 **粗体**、`inline code`、[链接](https://example.com) 应在当前栏宽内连续换行。'.repeat(4) + '正文结束标记。',
  '| 项目 | 验证 |\n| :-- | --: |\n| 窄栏 | 完整换行 |\n| 公式 | $E=mc^2$ |',
  '```typescript\nconst message = "' + 'long source '.repeat(14) + '";\nconsole.log(message);\n```',
  '> 引用文本\n>\n> - 第一项\n> - 第二项',
  '![本地图片](/fixture.png)',
  '最后一段 $\\frac{a+b}{2}$。',
].join('\n\n');

export async function runChatMarkdownCase(page, artifacts, sessionId) {
  const body = page.locator('[data-chat-markdown]').filter({ has: page.getByRole('heading', {name:'官方正文排版'}) });
  await body.waitFor();
  await body.locator('.katex').first().waitFor();
  assert.equal(await body.locator('table').count(), 1);
  assert.equal(await body.locator('blockquote li').count(), 2);
  const initial = await body.locator('.chat-markdown-content').boundingBox();
  await page.screenshot({path:join(artifacts,'markdown-wide.png')});
  await verifyMenuDockNavigation(page, body, artifacts);
  await body.getByRole('button', {name:'完整预览',exact:true}).click();
  await page.locator('[data-document-html-host] iframe').waitFor();
  await page.setViewportSize({width:1024,height:900});
  await page.waitForFunction(() => {
    const host = document.querySelector('[data-chat-markdown]');
    const p = host?.querySelector('p');
    return p && p.getBoundingClientRect().width < 400;
  });
  const geometry = await body.evaluate(host => {
    const content = host.querySelector('.chat-markdown-content');
    const paragraph = content.querySelector('p');
    const range = document.createRange(); range.selectNodeContents(paragraph);
    const rect = paragraph.getBoundingClientRect();
    return {width:rect.width,height:content.getBoundingClientRect().height,
      text:paragraph.textContent, right:rect.right,
      runs:[...range.getClientRects()].map(r => ({left:r.left,right:r.right,top:r.top,bottom:r.bottom}))};
  });
  assert.ok(geometry.text.endsWith('正文结束标记。'));
  assert.ok(geometry.height > initial.height, 'narrow body reserves the full wrapped height');
  assert.ok(geometry.runs.every(run => run.right <= geometry.right + 1), 'all inline text stays inside the conversation column');
  await page.screenshot({path:join(artifacts,'markdown-narrow-preview.png')});
  // A wheel over DOM prose still scrolls the GPUI transcript owner.
  const before = await body.locator('h2').boundingBox();
  const hit = await body.boundingBox();
  await page.mouse.move(hit.x + Math.min(100,hit.width/2), hit.y + 40);
  await page.mouse.wheel(0,300);
  await page.waitForFunction(top => document.querySelector('[data-chat-markdown] h2')?.getBoundingClientRect().top < top - 10, before.y);
  const wrap = body.getByRole('button', {name:'自动换行',exact:true});
  assert.equal(await wrap.getAttribute('aria-pressed'), 'true');
  await wrap.click();
  await page.waitForFunction(() => document.querySelector('[data-chat-markdown] button[aria-label="自动换行"]')?.getAttribute('aria-pressed') === 'false');
  assert.ok(await body.locator('pre').evaluate(pre => pre.scrollWidth > pre.clientWidth), 'unwrapped source scrolls horizontally');
  await wrap.click();
  await page.waitForFunction(() => document.querySelector('[data-chat-markdown] button[aria-label="自动换行"]')?.getAttribute('aria-pressed') === 'true');
  const code = await body.locator('pre').evaluate(pre => ({width:pre.clientWidth,scroll:pre.scrollWidth}));
  assert.ok(code.scroll <= code.width + 1, 'official wrap control wraps long code');
  await page.screenshot({path:join(artifacts,'markdown-code-wrapped.png')});
  const picture = body.getByRole('button', {name:'查看原图: 本地图片',exact:true});
  await picture.click();
  await page.getByRole('dialog', {name:'原图预览',exact:true}).waitFor();
  await page.keyboard.press('Escape');
  await page.getByRole('dialog', {name:'原图预览',exact:true}).waitFor({state:'detached'});
  // Session changes remove old DOM, without a delayed frame repainting it.
  await page.getByRole('button', {name:'打开 加载中的历史会话',exact:true}).dispatchEvent('click');
  await page.waitForFunction(() => !!window.__verification.finishColdHistory);
  await page.waitForFunction(() => !document.querySelector('[data-chat-markdown]'));
  await page.evaluate(() => window.__verification.finishColdHistory());
  await page.waitForFunction(() => document.querySelector('[data-chat-markdown]')?.dataset.chatMarkdown.startsWith('feedback-cold:'));
  await writeFile(join(artifacts,'markdown-geometry.json'),JSON.stringify({initial,geometry,code,sessionId},null,2));
  await verifyStreamingAdapter(page, artifacts);
  await verifyContextMenus(page, artifacts);
  await verifyImagePreviews(page, artifacts);
}

async function verifyImagePreviews(page, artifacts) {
  const test=await page.context().browser().newPage({viewport:{width:1100,height:900}});
  try {
    await test.route('**/markdown-image-test',route=>route.fulfill({contentType:'text/html',headers:{'Content-Security-Policy':nativeCsp},body:'<!doctype html><link rel="stylesheet" href="/trajectory-json.css"><body></body>'}));
    await test.goto(new URL('/markdown-image-test',page.url()).href);
    await test.evaluate(async bytes=>{
      window.__imageCalls=[];window.__imageCreated=[];window.__imageRevoked=[];
      const create=URL.createObjectURL.bind(URL),revoke=URL.revokeObjectURL.bind(URL);
      URL.createObjectURL=blob=>{const url=create(blob);window.__imageCreated.push(url);return url;};
      URL.revokeObjectURL=url=>{window.__imageRevoked.push(url);revoke(url);};
      window.__TAURI__={core:{invoke:async(name,args)=>{
        window.__imageCalls.push({name,args});
        if(name==='resolve_session_document')return{path:'/live/'+args.path,cwd:'/live'};
        if(name==='document_file'){
          if(args.path.includes('late.png'))await new Promise(resolve=>window.__finishImageRead=resolve);
          return{absolutePath:args.path,data:bytes};
        }
      }}};
      const adapter=await import('/trajectory-json.js');adapter.listenTrajectoryJson(()=>{});
      window.__imageRender=(session,text)=>{
        const source={session,text,streaming:false,compact:false,mentions:[],images:[]};
        const height=adapter.measureChatMarkdown('image-files',JSON.stringify(source),600,true);
        adapter.stageChatMarkdown('image-files',[30,30,630,30+height],[0,0,1100,900],false);
      };
      window.__imageRender('image-session','[图片文件](preview-only.png)\n\n![相对图片](nested/picture.png)');
    },[...await readFile(new URL('../src-tauri/icons/64x64.png',import.meta.url))]);
    await test.waitForFunction(()=>document.querySelector('[data-chat-markdown] img')?.naturalWidth===64);
    await test.getByRole('button',{name:'图片文件',exact:true}).focus();
    const preview=test.getByRole('img',{name:'preview-only.png',exact:true});
    await preview.waitFor();
    await test.waitForFunction(()=>[...document.images].some(image=>image.alt==='preview-only.png'&&image.naturalWidth===64));
    await test.screenshot({path:join(artifacts,'markdown-image-file-hover.png')});
    assert.ok((await test.evaluate(()=>window.__imageCalls)).some(call=>call.name==='document_file'&&call.args.path==='/live/nested/picture.png'));
    await test.evaluate(()=>window.__imageRender('late-session','[延迟图片](late.png)'));
    await test.waitForFunction(()=>!!window.__finishImageRead);
    await test.evaluate(()=>{window.__imageRender('new-session','新会话');window.__finishImageRead();});
    await test.waitForFunction(()=>window.__imageCreated.length===window.__imageRevoked.length);
    assert.equal(await test.locator('img').count(),0,'late old-session image is discarded and object URLs are released');
    await writeFile(join(artifacts,'markdown-image-file-reads.json'),JSON.stringify(await test.evaluate(()=>window.__imageCalls),null,2));
  } finally {await test.close();}
}

async function verifyMenuDockNavigation(page, body, artifacts) {
  const originalWidth = await body.evaluate(node=>node.getBoundingClientRect().width);
  const file=body.getByRole('button',{name:'完整预览',exact:true});
  await file.click({button:'right'});
  const menu=page.getByRole('menu');
  await menu.getByRole('menuitem',{name:'在浮窗中打开',exact:true}).click();
  const preview=page.locator('[data-document-html-host] iframe');
  await preview.waitFor();
  await page.waitForFunction(()=>document.querySelector('[data-document-html-host] iframe')?.getBoundingClientRect().width<390);
  assert.ok(Math.abs(await body.evaluate(node=>node.getBoundingClientRect().width)-originalWidth)<1,
    'opening a floating file preserves the closed sidebar and chat width');
  await page.screenshot({path:join(artifacts,'markdown-context-floating.png')});
  assert.equal(await page.evaluate(()=>document.elementsFromPoint(330,170).some(node=>node.closest('[data-chat-markdown]'))),false,'floating pane occludes prose and its hit targets');
  await page.mouse.move(320,140);await page.mouse.down();
  await page.mouse.move(1060,180,{steps:12});await page.mouse.up();
  await page.waitForFunction(()=>document.querySelector('[data-document-html-host] iframe')?.getBoundingClientRect().left>800);
  await file.click({button:'right'});
  await menu.getByRole('menuitem',{name:'在侧边栏打开',exact:true}).waitFor();
  await writeFile(join(artifacts,'markdown-menu-layer.json'),JSON.stringify(await menu.evaluate(node=>{
    const row=node.querySelector('button');const r=row.getBoundingClientRect();
    const inspect=e=>({tag:e.tagName,role:e.getAttribute('role'),class:e.className,style:e.getAttribute('style'),inert:e.inert,z:getComputedStyle(e).zIndex,pointer:getComputedStyle(e).pointerEvents});
    return {menu:inspect(node),row:inspect(row),rect:r.toJSON(),html:node.outerHTML,
      anchors:[...document.querySelectorAll('[style*="anchor-name"], [style*="anchorName"], [data-chat-context-menu]')].map(e=>({...inspect(e),rect:e.getBoundingClientRect().toJSON(),html:e.outerHTML.slice(0,500)})),
      hit:document.elementsFromPoint(r.x+r.width/2,r.y+r.height/2).map(inspect),ancestors:[node.parentElement,node.parentElement?.parentElement].filter(Boolean).map(inspect)};
  }),null,2));
  await page.screenshot({path:join(artifacts,'markdown-menu-floating-overlap.png')});
  await menu.getByRole('menuitem',{name:'在侧边栏打开',exact:true}).click({timeout:5000});
  await page.waitForFunction(()=>document.querySelector('[data-document-html-host] iframe')?.getBoundingClientRect().width>400);
}

async function verifyContextMenus(page, artifacts) {
  const test=await page.context().browser().newPage({viewport:{width:1100,height:900}});
  try {
    await test.route('**/markdown-menu-test',r=>r.fulfill({contentType:'text/html',headers:{'Content-Security-Policy':nativeCsp},body:'<!doctype html><link rel="stylesheet" href="/trajectory-json.css"><body></body>'}));
    await test.goto(new URL('/markdown-menu-test',page.url()).href);
    await test.evaluate(async()=>{
      window.__menuCalls=[];window.__menuActions=[];window.__menuCopied=[];
      const png='iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==';
      window.__TAURI__={core:{invoke:async(name,args)=>{
        window.__menuCalls.push({name,args});
        if(name==='resolve_session_document')return{path:args.path.startsWith('/')?args.path:'/live/'+args.path,cwd:'/live'};
        if(name==='document_file'){
          if(window.__menuDefer)await new Promise(resolve=>window.__menuResolve=resolve);
          if(args.path.includes('missing'))throw Error('Not found');
          return{absolutePath:args.path,data:[...Uint8Array.from(atob(png),c=>c.charCodeAt(0))]};
        }
        if(name==='document_applications')return[{id:'/Applications/Preview.app',name:'预览',default:true}];
        if(name==='save_file_bytes'){
          if(window.__menuSaveFail)throw Error('save failed');
          return args.mode==='saveAs'?{status:'cancelled'}:{status:'saved',path:'/Downloads/'+args.name};
        }
      }}};
      Object.defineProperty(navigator,'clipboard',{configurable:true,value:{
        writeText:async text=>window.__menuCopied.push(text),
        write:async items=>window.__menuImage=await (await items[0].getType('image/png')).arrayBuffer(),
      }});
      const adapter=await import('/trajectory-json.js');
      adapter.listenTrajectoryJson((action,value)=>{if(action==='chat-markdown')window.__menuActions.push(JSON.parse(value));});
      window.__menuRender=(session='menu-session')=>{
        adapter.beginTrajectoryJsonFrames();
        const data={session,text:'[网址](https://example.com/path) [文件](source.ts#L82) [缺失](missing.png)\n\n![图片](/original.png)',streaming:false,compact:false,mentions:[],images:[{authored:'/original.png',path:'/original.png',url:'data:image/png;base64,'+png}]};
        const h=adapter.measureChatMarkdown('menus',JSON.stringify(data),600,true);
        adapter.stageChatMarkdown('menus',[20,20,620,20+h],[0,0,1100,900],false);
      };
      window.__menuRender();
    });
    const menu=test.getByRole('menu');
    const file=test.getByRole('button',{name:'文件',exact:true});
    await test.getByRole('link',{name:'网址',exact:true}).click({button:'right'});
    await menu.getByRole('menuitem',{name:'复制链接地址',exact:true}).waitFor();
    assert.deepEqual(await menu.getByRole('menuitem').allTextContents(),['在侧边栏打开','外部浏览器打开','复制链接地址']);
    await menu.getByRole('menuitem',{name:'复制链接地址',exact:true}).click();
    assert.equal(await test.evaluate(()=>window.__menuCopied.at(-1)),'https://example.com/path');
    await file.focus();await test.keyboard.press('Shift+F10');
    await menu.getByRole('menuitem',{name:'用 预览 打开',exact:true}).waitFor();
    assert.deepEqual(await menu.getByRole('menuitem').allTextContents(),['在侧边栏打开','在浮窗中打开','在 Finder 中显示','用 预览 打开','复制文件路径','另存为…']);
    await test.screenshot({path:join(artifacts,'markdown-file-context-menu.png')});
    await menu.getByRole('menuitem',{name:'在浮窗中打开',exact:true}).click();
    assert.deepEqual(await test.evaluate(()=>window.__menuActions.at(-1)),{session:'menu-session',action:'file-menu',path:'source.ts',destination:'float',line:82});
    await file.click({button:'right'});await menu.getByRole('menuitem',{name:'复制文件路径',exact:true}).click();
    assert.equal(await test.evaluate(()=>window.__menuCopied.at(-1)),'/live/source.ts');
    const missing=test.getByRole('button',{name:'缺失',exact:true});
    await missing.click({button:'right'});
    await menu.getByRole('menuitem',{name:'复制图片',exact:true}).waitFor();
    assert.equal(await menu.getByRole('menuitem',{name:'在侧边栏打开',exact:true}).isDisabled(),true);
    await menu.getByRole('menuitem',{name:'复制文件路径',exact:true}).click();
    assert.equal(await test.evaluate(()=>window.__menuCopied.at(-1)),'/live/missing.png');
    const picture=test.getByRole('button',{name:'查看原图: 图片',exact:true});
    await picture.click({button:'right'});
    await menu.getByRole('menuitem',{name:'下载',exact:true}).waitFor();
    await test.screenshot({path:join(artifacts,'markdown-image-context-menu.png')});
    await menu.getByRole('menuitem',{name:'下载',exact:true}).click();
    await test.waitForFunction(()=>window.__menuCalls.some(c=>c.name==='save_file_bytes'));
    assert.equal(await test.evaluate(()=>window.__menuCalls.find(c=>c.name==='save_file_bytes').args.name),'original.png');
    assert.deepEqual(await test.evaluate(()=>window.__menuCalls.find(c=>c.name==='save_file_bytes').args.data.slice(0,8)),[137,80,78,71,13,10,26,10]);
    await picture.click({button:'right'});await menu.getByRole('menuitem',{name:'复制图片',exact:true}).click();
    await test.waitForFunction(()=>window.__menuImage?.byteLength>0);
    await picture.click({button:'right'});await menu.getByRole('menuitem',{name:'另存为…',exact:true}).click();
    await test.waitForFunction(()=>window.__menuCalls.some(c=>c.name==='save_file_bytes'&&c.args.mode==='saveAs'));
    assert.equal(await test.getByText('已保存',{exact:true}).count(),0,'cancellation does not announce a save');
    await test.evaluate(()=>window.__menuSaveFail=true);
    await picture.click({button:'right'});await menu.getByRole('menuitem',{name:'下载',exact:true}).click();
    await test.getByText('无法保存文件，请检查源文件和目标位置后重试',{exact:true}).waitFor();
    await file.focus();await test.keyboard.press('Shift+F10');await menu.waitFor();
    await test.keyboard.press('Escape');await menu.waitFor({state:'detached'});
    assert.equal(await file.evaluate(node=>node===document.activeElement),true,'Escape returns focus');
    await test.evaluate(()=>window.__menuDefer=true);
    await file.click({button:'right'});
    await test.waitForFunction(()=>!!window.__menuResolve);
    await test.evaluate(()=>{window.__menuRender('new-session');window.__menuResolve();});
    await test.waitForTimeout(1100);
    assert.equal(await menu.count(),0,'late metadata cannot reopen a menu from the previous session');
    await writeFile(join(artifacts,'markdown-context-menu-calls.json'),JSON.stringify(await test.evaluate(()=>({calls:window.__menuCalls,actions:window.__menuActions,copied:window.__menuCopied})),null,2));
  } finally {await test.close();}
}

async function verifyStreamingAdapter(page, artifacts) {
  const test = await page.context().browser().newPage({viewport:{width:1440,height:1100}});
  try {
    await test.route('**/markdown-adapter-test', route => route.fulfill({contentType:'text/html',body:'<!doctype html><link rel="stylesheet" href="/trajectory-json.css"><body></body>'}));
    await test.goto(new URL('/markdown-adapter-test',page.url()).href);
    await test.evaluate(async () => {
      const adapter = await import('/trajectory-json.js');
      adapter.listenTrajectoryJson(() => {});
      Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:async text => {window.__copiedMarkdown = text;}}});
      const prefix = '稳定段落一。\n\n稳定段落二。\n\n公式 $E=mc^2$ 和 [文档](source.ts#L82)。\n\n';
      const data = {session:'adapter-stream',text:prefix+'```ts\nconst a = 1;\n',streaming:true,compact:false,mentions:[],images:[]};
      const update = () => {
        const height = adapter.measureChatMarkdown('stream',JSON.stringify(data),600,true);
        adapter.stageChatMarkdown('stream',[20,20,620,20+height],[0,0,1440,1100],false);
        return height;
      };
      update();
      const first = document.querySelector('[data-chat-markdown] p');
      const pre = document.querySelector('[data-chat-markdown] pre');
      data.text += 'console.log(a);\n'; update();
      window.__markdownStreaming = {
        sameParagraph: first === document.querySelector('[data-chat-markdown] p'),
        sameFence: pre === document.querySelector('[data-chat-markdown] pre'),
        mathLiteral: !document.querySelector('.katex'),
        fileInert: ![...document.querySelectorAll('button')].some(button => button.textContent === '文档'),
      };
      data.text += '```'; data.streaming = false;
      window.__markdownSettledHeight = update();
    });
    assert.deepEqual(await test.evaluate(() => window.__markdownStreaming), {sameParagraph:true,sameFence:true,mathLiteral:true,fileInert:true});
    await test.locator('.katex').waitFor();
    await test.getByRole('button',{name:'文档',exact:true}).waitFor();
    await test.getByRole('button',{name:'复制',exact:true}).click();
    assert.equal(await test.evaluate(() => window.__copiedMarkdown), 'const a = 1;\nconsole.log(a);');
    await test.screenshot({path:join(artifacts,'markdown-stream-settled.png')});
    // A new frame hides stale placements synchronously, before its next paint.
    assert.equal(await test.evaluate(async () => {
      const adapter = await import('/trajectory-json.js'); adapter.beginTrajectoryJsonFrames();
      return getComputedStyle(document.querySelector('[data-chat-markdown]')).visibility;
    }), 'hidden');
  } finally { await test.close(); }
}
