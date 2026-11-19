import assert from 'node:assert/strict';
import { planFixture, runPlanPreviewCase } from './plan-preview-browser.mjs';
import { runDocumentAndQuestionCase } from './document-question-browser.mjs';
import { markdownFixture, runChatMarkdownCase } from './chat-markdown-browser.mjs';
import { createServer } from 'node:http';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inflateSync } from 'node:zlib';
import { chromium, webkit } from 'playwright-core';
import { runOutputBlocksCase } from './trajectory-blocks-browser.mjs';
import { runFontsCase } from './settings-fonts-browser.mjs';
import { installModelsMock, runModelsCase } from './models-settings-browser.mjs';
import { installInventoryMock, runInventoryCase } from './plugin-inventory-browser.mjs';
import { installDeveloperMock, runDeveloperCase } from './settings-developer-browser.mjs';
import { installAgentPresetsMock, runAgentPresetsCase } from './settings-agent-presets-browser.mjs';
import { installPermissionMock, runPermissionCase } from './settings-permission-browser.mjs';
import { installUploadMock, runUploadCase } from './settings-upload-browser.mjs';

const root = dirname(fileURLToPath(new URL('../package.json', import.meta.url)));
process.chdir(root);
const modeIndex = process.argv.indexOf('--mode');
const mode = modeIndex >= 0 ? process.argv[modeIndex + 1] : 'all';
assert.ok(['document-question', 'plan-preview', 'chat-markdown', 'file-links', 'ui-feedback', 'agent-presets', 'trajectory-blocks', 'settings-developer', 'fonts', 'settings-permission', 'settings-upload', 'plugin-inventory', 'models-official', 'all', 'image', 'inherited', 'theme', 'stream', 'turn-navigation', 'trajectory', 'trajectory-results', 'trajectory-program', 'trajectory-folds', 'trajectory-menus', 'trajectory-long', 'trajectory-narrow', 'trajectory-pages', 'trajectory-prompts', 'trajectory-timeline', 'settings', 'settings-shell', 'settings-partial', 'settings-readonly', 'settings-search', 'settings-fetch', 'appearance', 'plugins', 'workflow', 'team', 'permission', 'approval', 'queue', 'composer', 'composer-focus', 'attachments', 'welcome', 'provider-settings', 'provider-protocols', 'provider-authorization', 'deepseek-account', 'desktop-onboarding', 'account-notifications', 'account-quota', 'model-menu', 'model-catalog', 'model-accessibility', 'goal', 'reading', 'session-order', 'workspace-registry', 'workspace-actions', 'workspace-startup', 'workspace-search', 'workspace-accessibility', 'workspace-group-accessibility'].includes(mode), `unknown mode: ${mode}`);
const artifactArgument = process.argv.slice(2).find(argument => argument !== '--mode' && argument !== mode);
const artifacts = resolve(artifactArgument || join(root, '.tools/verification/markdown-tools-browser', mode));
await mkdir(artifacts, { recursive: true });
const sessionId = 'session-markdown-tools-browser';
const cwd = root;
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.png': 'image/png', '.svg': 'image/svg+xml', '.ttf': 'font/ttf' };
const nativeCsp = JSON.parse(await readFile(join(root, 'src-tauri/tauri.conf.json'), 'utf8')).app.security.csp;
const server = createServer(async (request, response) => {
  try {
    const relative = decodeURIComponent(new URL(request.url, 'http://localhost').pathname).replace(/^\/+/, '') || 'index.html';
    const path = join(root, 'ui/dist', normalize(relative));
    if (!path.startsWith(join(root, 'ui/dist') + sep)) throw new Error('invalid path');
    const body = await readFile(path);
    response.writeHead(200, { 'Content-Type': mime[extname(path)] || 'application/octet-stream',
      ...(['chat-markdown', 'document-question', 'plan-preview'].includes(mode) ? { 'Content-Security-Policy': nativeCsp } : {}) });
    response.end(body);
  } catch (error) {
    if (response.headersSent) response.destroy(error);
    else { response.writeHead(404); response.end(String(error)); }
  }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const useWebKit = process.env.KWORK_TEST_BROWSER === 'webkit';
const engine = useWebKit ? webkit : chromium;
const browserServer = await engine.launchServer({
  host: '127.0.0.1',
  executablePath: useWebKit ? undefined : process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true,
  args: useWebKit ? [] : ['--no-first-run', '--enable-unsafe-webgpu', '--use-angle=swiftshader',
    ...(mode === 'trajectory-menus' ? ['--force-device-scale-factor=2'] : [])],
});
const browser = await engine.connect(browserServer.wsEndpoint());
const page = await browser.newPage({ viewport: { width: 1440, height: 1100 }, deviceScaleFactor: mode === 'trajectory-menus' ? 2 : 1 });
const legacySettings = ['settings', 'settings-partial', 'settings-readonly', 'settings-search', 'settings-fetch'].includes(mode);
const platform = process.env.KWORK_TEST_PLATFORM ?? (legacySettings ? 'Win32' : undefined);
if (platform) {
  await page.addInitScript(platform => Object.defineProperty(navigator, 'platform', { value: platform }), platform);
}
const errors = [];
page.on('pageerror', error => { errors.push(String(error)); console.error('PAGE ERROR', error); });
page.on('console', message => {
  if (message.type() === 'error') {
    errors.push(message.text()); console.error('CONSOLE', message.text());
  }
  if (message.text().startsWith('web settings')) console.log(message.text());
});
const png = (await readFile(join(root, 'src-tauri/icons/64x64.png'))).toString('base64');
const imageAttachment = { attachmentId: 'a'.repeat(64), mediaType: 'image/png', bytes: Buffer.from(png, 'base64').length, width: 64, height: 64, name: 'fixture.png' };
const turnStart = { type: 'turn/start', seq: 1, data: { turn: 1 } };
const userMessage = { type: 'user/message', seq: 2, surfaceOp: 'append', data: { id: 'user', source: { kind: 'user' }, role: 'user', content: [{ type: 'text', text: '请验证公式、差异和图片' }] } };
const turnEnd = { type: 'turn/end', seq: 10, data: { turn: 1, reason: { kind: 'completed' } } };
const longTrajectoryEvents = Array.from({ length: 30 }, (_, index) => {
  const seq = index + 1;
  const time = 1000 + index * 200;
  return { type: 'tool/call', seq, time, data: { turn: 1, step: 1, callId: `long-${seq}`, name: 'bash', arguments: JSON.stringify({ command: `step-${String(seq).padStart(2, '0')}` }) } };
});
function paginatedToolCall(seq, label) {
  return { type: 'tool/call', seq, time: seq * 200, data: { turn: 1, step: 1, callId: `page-${seq}`, name: 'bash', arguments: JSON.stringify({ command: `${label}-${seq}` }) } };
}
const imageEvents = [
  { type: 'tool/call', seq: 6, data: { callId: 'image', name: 'read_image', arguments: JSON.stringify({ file_path: 'fixture.png' }) } },
  { type: 'tool/result', seq: 7, surfaceOp: 'append', data: { message: { toolCallId: 'image', content: [
    { type: 'text', text: '<path>/fixture.png</path>\n<type>image</type>\n<content>\nfixture\n</content>' },
    { type: 'image', attachment: imageAttachment },
    { type: 'text', text: '图片附加说明' },
  ], isError: false }, meta: { path: '/fixture.png' } } },
];
const contentEvents = [
  { type: 'assistant/message', seq: 3, surfaceOp: 'append', data: { message: { id: 'assistant', content: [
    { type: 'text', text: '行内公式 $E=mc^2$ 与 \\(\\alpha+\\beta\\)。\n\n$$\n\\frac{a+b}{2}\n$$\n\n\\[\n\\sqrt{x^2+y^2}\n\\]\n\n```math\n\\begin{bmatrix}a&b\\\\c&d\\end{bmatrix}\n```\n\n非法公式 $\\frac{1$ 与 `$code$` 保留原文。\n\n```rust\nfn main() {\n    println!("hello");\n}\n```' },
  ] } } },
  { type: 'tool/call', seq: 4, data: { callId: 'edit', name: 'edit', arguments: JSON.stringify({ file_path: 'src/main.rs', old_string: 'let a = 1;', new_string: 'let a = 2;' }) } },
  { type: 'tool/result', seq: 5, surfaceOp: 'append', data: { message: { toolCallId: 'edit', content: [{ type: 'text', text: 'updated' }], isError: false }, meta: { path: join(root, 'src/main.rs'), diffs: [{ path: join(root, 'src/main.rs'), oldText: 'fn main() {\n    let a = 1;\n}', newText: 'fn main() {\n    let a = 2;\n    println!("start");\n    println!("middle one");\n    println!("middle two");\n    println!("middle three");\n    println!("middle four");\n    println!("end");\n}' }] } } },
  { type: 'tool/call', seq: 8, data: { callId: 'web', name: 'web_fetch', arguments: JSON.stringify({ url: 'https://example.com' }) } },
  { type: 'tool/result', seq: 9, surfaceOp: 'append', data: { message: { toolCallId: 'web', content: [{ type: 'text', text: 'Error: web fetch failed: could not resolve the host name (ENOTFOUND): getaddrinfo ENOTFOUND example.com' }], isError: true }, error: { name: 'WebError', code: 'WEB_FETCH_DNS_FAILED' } } },
];
const themeEvents = [
  turnStart,
  userMessage,
  { type: 'assistant/message', seq: 3, surfaceOp: 'append', data: { message: { id: 'assistant', content: [{ type: 'text', text: '主题与字号验收：行内公式 $E=mc^2$，代码 `fn main()` 应随主题和字号设置正确呈现。' }] } } },
  turnEnd,
];
const programSource = 'const literal = "<b> & 中文";\n' + Array.from({ length: 65 }, (_, i) => `const line${i} = "${'long source '.repeat(12)}";`).join('\n') + '\nreturn { literal };\n';
const programArguments = JSON.stringify({ code: programSource, description: 'Inspect structured source' }, null, 3);
const blockCases = [
  { name:'json_single', blocks:[{type:'text',text:'{"a":1}'}] },
  { name:'json_split', blocks:[{type:'text',text:'{"a":'},{type:'text',text:'1}'}] },
  { name:'mixed_image', blocks:[{type:'text',text:'before <b>literal</b>'},{type:'image',attachment:imageAttachment},{type:'text',text:'after\n'}] },
  { name:'error_image', blocks:[{type:'image',attachment:{...imageAttachment,attachmentId:'b'.repeat(64),name:'retry.png'}}], error:{name:'ToolError',code:'E_IMAGE'} },
  { name:'file_result', blocks:[{type:'file',attachment:{attachmentId:'c'.repeat(64),mediaType:'text/plain',name:'report.txt',bytes:4}}] },
];
const blockEvents = [turnStart,userMessage,...blockCases.flatMap((item,index)=>[
  {type:'tool/call',seq:3+index*2,time:1000+index*200,data:{turn:1,step:1,callId:item.name,name:item.name,arguments:'{"path":"fixture"}'}},
  {type:'tool/result',seq:4+index*2,time:1100+index*200,data:{turn:1,step:1,error:item.error,message:{source:{callId:item.name},isError:!!item.error,content:item.blocks}}},
]),{...turnEnd,seq:20}];
const events = mode === 'inherited'
  ? [
      { type: 'user/message', seq: 1, data: { id: 'parent', content: [{ type: 'text', text: '继承的父会话消息' }] } },
      { type: 'assistant/message', seq: 2, data: { turn: 1, step: 1, message: { id: 'parent-reasoning', content: [{ type: 'reasoning', text: '继承的思考' }] } } },
      { type: 'session/end-seed', seq: 3, data: { inherited: true } },
      { type: 'assistant/message', seq: 4, data: { turn: 1, step: 1, message: { id: 'child-answer', content: [{ type: 'text', text: '分叉后的新回复' }] } } },
      { type: 'turn/end', seq: 5, data: { turn: 1, reason: { kind: 'forked' } } },
    ]
  : mode === 'document-question' ? [turnStart, userMessage,
    {type:'assistant/message',seq:3,surfaceOp:'append',data:{message:{id:'preview',content:[{type:'text',text:'[打开文档](docs/guide.md)'}]}}}, turnEnd]
  : mode === 'plan-preview' ? [turnStart, userMessage,
    { type: 'tool/call', seq: 3, data: { callId: 'fixture-plan', name: 'exit_plan_mode', arguments: JSON.stringify({plan: planFixture}) } },
    {...turnEnd, gpuiSubmittedPlans: [{callId: 'fixture-plan', title: '官方计划预览', markdown: planFixture}]}]
  : mode === 'chat-markdown' ? [turnStart, userMessage,
    {type:'assistant/message',seq:3,surfaceOp:'append',data:{message:{id:'markdown',content:[{type:'text',text:markdownFixture}]}}}, turnEnd]
  : mode === 'file-links' ? [turnStart, userMessage,
    { type: 'assistant/message', seq: 3, surfaceOp: 'append', data: { message: { id: 'file-links', content: [{ type: 'text', text:
      '[预览完整 HTML](完整预览.html)\n\n[跳到第 82 行](source.ts#L82)\n\n[回到第 3 行](./source.ts:3:2)\n\n[工作区外文档](../outside.txt#L4)\n\n[编码文件名](notes%23draft%3F.txt)\n\n[不能执行](javascript:alert)'
    }] } } }, turnEnd]
  : mode === 'ui-feedback' ? [turnStart, userMessage,
    { type: 'tool/call', seq: 3, data: { callId: 'write-preview', name: 'write', arguments: JSON.stringify({ file_path: join(cwd, '完整预览.html'), content: '<h1>完整预览</h1>' }) } },
    { type: 'tool/result', seq: 4, surfaceOp: 'append', data: { message: { toolCallId: 'write-preview', content: [{ type: 'text', text: 'written' }] } } },
    { type: 'assistant/message', seq: 5, surfaceOp: 'append', data: { message: { id: 'feedback-text', content: [{ type: 'text',
      text: '点击 `完整预览.html`，直接改它就能重新排盘。' + '这段文字必须在打开右侧预览后完整换行，不能被右侧面板遮住。'.repeat(5) + '正文结束标记。' }] } } }, turnEnd]
  : mode === 'trajectory-blocks' ? blockEvents
  : mode === 'account-quota'
  ? [turnStart, userMessage, { ...turnEnd, data: { turn: 1, reason: { kind: 'error', error: { code: 'ACCOUNT_QUOTA', message: 'Historical failure' } } } }]
  : mode === 'image'
  ? [turnStart, userMessage, ...imageEvents, turnEnd]
  : mode === 'turn-navigation'
    ? [turnStart, userMessage, turnEnd,
      { type: 'turn/start', seq: 11, data: { turn: 2 } },
      { type: 'user/message', seq: 12, surfaceOp: 'append', data: {
        ...userMessage.data, id: 'second', content: [{ type: 'text', text: '第二轮导航验收' }],
      } },
      { ...turnEnd, seq: 13, data: { ...turnEnd.data, turn: 2 } },
    ]
  : mode === 'workflow'
    ? [
      turnStart, userMessage,
      { type: 'tool-workflow/run-start', seq: 3, data: { runId: 7, name: '发布检查' } },
      { type: 'tool-workflow/agent-start', seq: 4, data: { runId: 7, seq: 1, label: '检查 API', childId: 'child-api', phase: '实现' } },
      { type: 'subagent/catalog', seq: 5, data: { childId: 'child-api', label: '检查 API', mode: 'one-shot', childCreatedAt: 1 } },
      { type: 'tool-workflow/agent-start', seq: 6, data: { runId: 7, seq: 2, label: '', childId: 'child-test', phase: '' } },
      { type: 'tool-workflow/agent-end', seq: 7, data: { runId: 7, seq: 1, outcome: 'completed' } },
    ]
  : mode === 'team'
    ? [
      { type: 'team/mode', seq: 1, data: { enabled: false } },
      turnStart, userMessage,
      { type: 'team/task', seq: 4, data: { version: 2, teamId: sessionId, task: { id: 'task-api', revision: 1, subject: '实现 API', description: '补齐接口和测试', status: 'in_progress', blockedBy: [], ownerId: 'child-api', ownerName: 'API Agent' } } },
      turnEnd,
    ]
  : mode === 'trajectory-prompts'
    ? [
      { type: 'turn/start', seq: 1, time: 1000, data: { turn: 1 } },
      { type: 'user/message', seq: 2, time: 1100, surfaceOp: 'append', data: { id: 'prompt-user', source: { kind: 'user' }, content: [{ type: 'text', text: 'Inspect prompt history' }] } },
      { type: 'system/message', seq: 3, time: 1200, surfaceOp: 'append', data: { turn: 1, step: 1, message: { content: [{ type: 'text', text: '# 工作规则\n\n保留原始内容。\n' }] } } },
      { type: 'step/start', seq: 4, time: 1250, data: { turn: 1, step: 1 } },
      { type: 'request/header', seq: 5, time: 1300, data: { reason: 'initial', header: { config: { provider: 'deepseek', model: 'v4' }, tools: [{ name: 'bash', description: '执行命令并返回结果。', parameters: { type: 'object', properties: { command: { type: 'string' } } } }] } } },
      { type: 'system/message', seq: 6, time: 1400, surfaceOp: 'append', data: { turn: 1, step: 1, message: { content: [{ type: 'text', text: '# 工作规则\n\n保留完整内容。\n' }] } } },
      { type: 'request/header', seq: 7, time: 1500, data: { reason: 'tools', header: { config: { provider: 'deepseek', model: 'v4' }, tools: [{ name: 'bash', description: '执行命令并返回结果。', parameters: { type: 'object', properties: { command: { type: 'string' } } } }, { name: 'read_file', description: '读取文件。\n保留原始换行。', parameters: { type: 'object', properties: { path: { type: 'string' } } } }] } } },
      { type: 'system/message', seq: 8, time: 1600, surfaceOp: { startSeq: 6, endSeq: 6 }, data: { turn: 1, step: 1, message: { content: [{ type: 'text', text: '# 更新后的规则\n\n没有可用工具。\n' }] } } },
      { type: 'request/header', seq: 9, time: 1700, data: { reason: 'series', header: { config: { provider: 'deepseek', model: 'v4' }, tools: [] } } },
      { type: 'user/message', seq: 10, time: 1800, surfaceOp: { startSeq: 3, endSeq: 8 }, data: { source: { kind: 'compact-checkpoint' }, content: [] } },
      { type: 'step/end', seq: 11, time: 1900, data: { turn: 1, step: 1 } },
    ]
  : mode === 'trajectory-pages'
    ? [
      { type: 'turn/start', seq: 300, time: 60000, data: { turn: 1 } },
      { type: 'user/message', seq: 301, time: 60100, data: { id: 'page-user', content: [{ type: 'text', text: 'Paginated trajectory' }] } },
      { type: 'step/start', seq: 302, time: 60200, data: { turn: 1, step: 1 } },
      ...Array.from({ length: 180 }, (_, index) => paginatedToolCall(index + 303, 'recent')),
    ]
  : mode === 'trajectory-long' || mode === 'trajectory-timeline'
    ? [
      { type: 'turn/start', seq: 1, time: 800, data: { turn: 1 } },
      { type: 'user/message', seq: 2, time: 900, data: { id: 'long-user', source: { kind: 'user' }, content: [{ type: 'text', text: 'Long trajectory' }] } },
      { type: 'step/start', seq: 3, time: 950, data: { turn: 1, step: 1 } },
      ...longTrajectoryEvents.map(event => ({ ...event, seq: event.seq + 3 })),
    ]
  : ['trajectory-folds', 'trajectory-menus'].includes(mode)
    ? [
      { type: 'turn/start', seq: 1, time: 1000, data: { turn: 1 } },
      { type: 'user/message', seq: 2, time: 1100, data: { id: 'fold-user', content: [{ type: 'text', text: 'Inspect calls' }] } },
      { type: 'step/start', seq: 3, time: 1200, data: { turn: 1, step: 1 } },
      { type: 'assistant/message', seq: 4, time: 1300, data: { turn: 1, step: 1, message: { id: 'fold-assistant', content: [{ type: 'text', text: 'Working' }] } } },
      { type: 'tool/call', seq: 5, time: 1400, data: { turn: 1, step: 1, callId: 'root', name: 'run_code', arguments: '{}' } },
      { type: 'tool/ptc-dispatch-start', seq: 6, time: 1500, data: { rootCallId: 'root', parentCallId: 'root', subCallId: 'child', name: 'read_file', arguments: { path: 'README.md' } } },
      { type: 'tool/ptc-dispatch-start', seq: 7, time: 1600, data: { rootCallId: 'root', parentCallId: 'child', subCallId: 'grandchild', name: 'grep', arguments: { query: 'needle' } } },
      { type: 'tool/call', seq: 8, time: 1700, data: { turn: 1, step: 1, callId: 'leaf', name: 'bash', arguments: '{}' } },
      { type: 'assistant/message', seq: 9, time: 1800, data: { turn: 1, step: 1, message: { id: 'fold-end', content: [{ type: 'text', text: 'Done' }] } } },
    ]
  : ['trajectory-program', 'fonts'].includes(mode)
    ? [
      { type: 'turn/start', seq: 1, time: 1000, data: { turn: 1 } },
      { type: 'user/message', seq: 2, time: 1100, data: { id: 'program-user', content: [{ type: 'text', text: 'Inspect program calls' }] } },
      { type: 'step/start', seq: 3, time: 1200, data: { turn: 1, step: 1 } },
      { type: 'request/header', seq: 4, time: 1250, data: { header: { config: {}, tools: [
        { name: 'run_code', description: 'Execute a program in `TypeScript`', parameters: { type: 'object' } },
        { name: 'cua_run_code', description: 'Execute a program in `python`', parameters: { type: 'object' } },
      ] } } },
      { type: 'tool/call', seq: 5, time: 1300, data: { turn: 1, step: 1, callId: 'program', name: 'run_code', arguments: programArguments } },
      { type: 'tool/result', seq: 6, time: 1400, data: { message: { toolCallId: 'program', content: [{ type: 'text', text: '{"output":[false,null,7]}' }] } } },
      { type: 'tool/call', seq: 7, time: 1500, data: { turn: 1, step: 1, callId: 'python', name: 'cua_run_code', arguments: JSON.stringify({ code: 'print("hello")\n', description: 'Python failure' }) } },
      { type: 'tool/result', seq: 8, time: 1600, data: { message: { toolCallId: 'python', isError: true, content: [{ type: 'text', text: 'Error: example failure\nCaptured output:\n<b>literal</b>' }] } } },
      { type: 'tool/call', seq: 9, time: 1700, data: { turn: 1, step: 1, callId: 'malformed', name: 'run_code', arguments: '{"code":"x","description":null}' } },
      { type: 'tool/call', seq: 10, time: 1800, data: { turn: 1, step: 1, callId: 'running', name: 'run_code', arguments: '{"code":"await pending();","description":"Still running"}' } },
    ]
  : ['trajectory', 'trajectory-narrow', 'trajectory-results'].includes(mode)
    ? [
      { type: 'turn/start', seq: 1, time: 1000, data: { turn: 1 } },
      { type: 'user/message', seq: 2, time: 1100, data: { id: 'u', source: { kind: 'user', zeta: 'first', alpha: 'last' }, content: [{ type: 'text', text: 'Inspect this' }] } },
      { type: 'step/start', seq: 3, time: 1200, data: { turn: 1, step: 1 } },
      { type: 'assistant/message', seq: 4, time: 1600, data: { turn: 1, step: 1, message: { id: 'a', source: { provider: 'deepseek', model: 'v4' }, content: [{ type: 'reasoning', text: 'Think' }, { type: 'text', text: 'Done' }, { type: 'tool-call', id: 'c', name: 'bash', arguments: '{}' }] }, usage: { inputTokens: 20, outputTokens: 5 }, stream: [{ type: 'text-chunks', time0: 1350, text: 'Done' }] } },
      { type: 'tool/call', seq: 5, time: 1700, data: { turn: 1, step: 1, callId: 'c', name: 'bash', arguments: JSON.stringify({ command: 'pwd', options: { cwd: '/tmp', nested: [{ enabled: true, count: 7 }] }, empty: {}, note: '长文本保留\n'.repeat(240) + 'END\n' }) } },
      { type: 'tool/result', seq: 6, time: 1900, data: { turn: 1, step: 1, message: { source: { callId: 'c' }, content: [{ type: 'text', text: '/tmp' }] } } },
      { type: 'step/end', seq: 7, time: 2000, data: { turn: 1, step: 1 } },
      { type: 'step/start', seq: 8, time: 2100, data: { turn: 1, step: 2 } },
      ...(mode === 'trajectory-results' ? [
        { type: 'tool/call', seq: 9, time: 2200, data: { turn: 1, step: 2, callId: 'other', name: 'read_file', arguments: '{"path":"fixture.txt"}' } },
        { type: 'tool/result', seq: 10, time: 2300, data: { turn: 1, step: 2, message: { toolCallId: 'other', content: [{ type: 'text', text: 'Other rendered result\n<b>literal</b>\n```js\nconst x = 7;\n```\n' }] } } },
        { ...turnEnd, seq: 11 },
      ] : [turnEnd]),
    ]
  : ['theme', 'stream', 'settings', 'settings-partial', 'settings-readonly', 'settings-search', 'settings-fetch', 'appearance', 'plugins', 'workflow', 'team', 'permission', 'approval', 'queue', 'composer', 'composer-focus', 'attachments', 'model-menu', 'model-catalog', 'model-accessibility', 'goal', 'reading', 'session-order', 'workspace-registry', 'workspace-actions', 'workspace-startup', 'workspace-search', 'workspace-accessibility', 'workspace-group-accessibility'].includes(mode)
    ? themeEvents
    : [turnStart, userMessage, ...contentEvents, ...imageEvents, turnEnd];
if (mode === 'document-question') {
  events.push({
    type: 'user/message', seq: events.at(-1).seq + 1, time: 10000, surfaceOp: 'append',
    data: {
      id: 'settled-question', source: { kind: 'user-question-reply', outcome: 'answered', callId: 'previous-question' },
      content: [{ type: 'text', text: JSON.stringify({
        questions: [{ id: 'previous', question: '历史提问：保留哪些内容？' }],
        answers: [{ id: 'previous', selected: [], custom: '保留完整的历史回答。' }],
      }) }],
    },
  });
}
const history = { events, hasMore: mode === 'trajectory-pages', sessionStats: null, tokenUsage: null };
let verificationFailed = false;
try {
  const orderWorkspace = join(cwd, 'Fixture workspace');
  const outsiderId = 'workspace-unaccounted-session';
  const hostWorkspaces = [
    { id: 'host-sibling', path: join(cwd, '..', 'Host sibling'), title: 'Host sibling', sessionIds: [] },
    { id: 'host-parent', path: cwd, title: 'Host parent', sessionIds: mode === 'workspace-search'
      ? [sessionId, 'search-41abc', 'archived-match', ...Array.from({ length: 52 }, (_, i) => `search-extra-${i}`)]
      : ['workspace-actions', 'workspace-group-accessibility'].includes(mode) ? [sessionId, outsiderId] : [sessionId] },
    { id: 'host-child', path: orderWorkspace, title: 'Host child', sessionIds: [] },
  ];
  const chosenDirectory = join(cwd, '..', 'Picker alias');
  const canonicalDirectory = join(cwd, '..', 'Canonical picker');
  const defaultPath = join(cwd, '..', 'Documents', 'deepseek-harness', 'default-workspace');
  const defaultWorkspace = { id: 'default-registration', path: defaultPath, title: 'default-workspace', sessionIds: [] };
  if (['models-official', 'settings-shell'].includes(mode)) await installModelsMock(page, JSON.parse(await readFile(process.env.KWORK_MODELS_FIXTURE || new URL('./fixtures/models-settings.json', import.meta.url), 'utf8')));
  if (mode === 'agent-presets') await installAgentPresetsMock(page);
  if (mode === 'plugin-inventory') await installInventoryMock(page);
  if (mode === 'settings-developer') await installDeveloperMock(page);
  if (mode === 'settings-permission') await installPermissionMock(page);
  if (mode === 'settings-upload') await installUploadMock(page);
  await page.addInitScript(({ sessionId, cwd, history, png, attachmentId, mode, orderWorkspace, hostWorkspaces, outsiderId, chosenDirectory, canonicalDirectory, defaultWorkspace, theme }) => {
    const workspaceRegistry = ['workspace-registry', 'workspace-actions', 'workspace-search', 'workspace-accessibility', 'workspace-group-accessibility'].includes(mode);
    const startup = mode === 'workspace-startup' ? new URLSearchParams(location.search).get('scenario') : null;
    const welcomeCase = mode === 'welcome' ? new URLSearchParams(location.search).get('scenario') : null;
    const accountScenario = ['deepseek-account', 'account-quota'].includes(mode) && new URLSearchParams(location.search).get('scenario');
    const onboardingScenario = mode === 'desktop-onboarding' && new URLSearchParams(location.search).get('scenario');
    const noticeIntro = ['account-notifications', 'account-quota'].includes(mode) && new URLSearchParams(location.search).get('scenario') === 'intro';
    const restoredAccount = ['restored', 'legacy'].includes(accountScenario) || mode === 'account-quota'
      || mode === 'desktop-onboarding' && onboardingScenario !== 'api-key' || mode === 'account-notifications';
    const onboardingKey = `desktop-onboarding-fixture:${onboardingScenario || 'default'}`;
    const newProgress = { version: 1, step: 'welcome', purpose: null, process: null, completion: null,
      usage: 'compact', developerTools: false };
    const onboardingDone = { revision: 1, progress: { ...newProgress, step: 'done', process: 'standard',
      completion: 'api-key', usage: 'detailed', developerTools: !['ui-feedback', 'file-links', 'chat-markdown'].includes(mode) } };
    const onboardingSeed = onboardingScenario === 'office' ? { step: 'purpose' }
      : onboardingScenario === 'skip' ? { step: 'credit' } : {};
    let onboardingSnapshot = mode === 'desktop-onboarding'
      ? JSON.parse(sessionStorage.getItem(onboardingKey)) || { revision: 0, progress: { ...newProgress, ...onboardingSeed } }
      : noticeIntro ? { revision: 0, progress: newProgress } : onboardingDone;
    const startupWorkspaces = startup === 'recent' || startup === 'restore' ? hostWorkspaces.map(item =>
      item.id === 'host-child' ? { ...item, sessionIds: ['recent-session'] } : item) : [];
    const startupSessions = startup === 'recent' || startup === 'restore' || startup === 'history'
      ? [{ sessionId, cwd, title: 'Earlier conversation', createdAt: 1000 },
        ...(startup === 'history' ? [] : [{ sessionId: 'recent-session', cwd: orderWorkspace, title: 'Most recent conversation', createdAt: 2000 }])]
      : [];
    const calls = [];
    const listeners = [];
    const goal = { id: 'fixture-goal', revision: 1, objective: '原始目标', phase: 'paused', activation: 'disarmed' };
    const orderSessions = [
      ['pin-alpha', 'Pinned Alpha', cwd, 900],
      ['first', 'First session', cwd, 550],
      ['archived', 'Archived session', cwd, 750],
      ['other-first', 'Other first', orderWorkspace, 700],
      ['second', 'Second session', cwd, 600],
      ['pin-beta', 'Pinned Beta', cwd, 500],
      ['other-second', 'Other second', orderWorkspace, 400],
    ].map(([sessionId, title, cwd, createdAt]) => ({ sessionId, title, cwd, createdAt }));
    const searchSessions = [
      { sessionId, cwd, title: '公式与工具结果验收', createdAt: 1 },
      ...Array.from({ length: 23 }, (_, i) => ({
        sessionId: `search-index-${i}`, cwd, title: `Index fixture ${i}`, createdAt: i + 100,
      })),
      ...Array.from({ length: 52 }, (_, i) => ({
        sessionId: `search-extra-${i}`, cwd, title: `Additional workspace row ${i}`, createdAt: i + 1000,
      })),
      { sessionId: 'index-suffix', cwd, title: 'Review index notes', createdAt: 2100 },
      { sessionId: 'index-newest', cwd, title: 'Index launch plan', createdAt: 2000 },
      { sessionId: 'search-41abc', cwd, title: 'Review workflow', createdAt: 800 },
      { sessionId: 'ranking-newer', cwd, title: 'Run smoke tests', createdAt: 900 },
      { sessionId: 'ranking-older', cwd, title: 'Registry state transition', createdAt: 850 },
      { sessionId: 'kana-clear', cwd, title: 'か', createdAt: 920 },
      { sessionId: 'kana-voiced', cwd, title: 'が', createdAt: 930 },
      { sessionId: 'unregistered', cwd: `${cwd}/Secret-ancestor/ActualLeaf`, title: 'Independent', createdAt: 5 },
      { sessionId: 'needle-title', cwd, title: 'Needle index', createdAt: 2 },
      { sessionId: 'durable-blank', cwd, title: 'Needle blank', createdAt: 9999, blank: true },
      { sessionId: 'content-one', cwd, title: 'Content one', createdAt: 8 },
      { sessionId: 'content-two', cwd, title: 'Content two', createdAt: 12 },
      { sessionId: 'archived-match', cwd, title: 'Needle of archive', createdAt: 5000 },
    ];
    const searchRequests = new Map();
    const route = (provider, model) => JSON.stringify([provider, model]);
    const baseModel = route('custom-host', 'base');
    const fallbackModel = route('custom-host', 'replay');
    const flashModel = route('deepseek-official', 'official-matrix-fast');
    const customGroup = { group: 'custom-host', name: 'Fixture custom', options: [
      { value: baseModel, name: 'Base Model' },
      { value: fallbackModel, name: 'Stable Replay', description: 'Routed telemetry with a wider context window' },
      { value: route('custom-host', 'custom-matrix-fast'), name: 'Bridge Speed' },
    ] };
    const officialGroup = { group: 'deepseek-official', name: 'Fixture official', options: [
      { value: flashModel, name: 'Flash Trial' },
      { value: route('deepseek-official', 'native-9a'), name: 'Long Reasoner' },
    ] };
    let currentModel = baseModel;
    let currentEffort = '';
    let currentPermission = 'read-only';
    const permissionOptions = [
      { value: 'read-only', name: 'Read only' },
      { value: 'workspace-write', name: 'Workspace write', description: '仅在工作区内修改文件' },
      { value: 'danger-full-access', name: 'Full access' },
      { value: 'auto', name: 'Auto review' },
    ];
    const modelConfig = ({ groups = [customGroup, officialGroup], current = currentModel } = {}) => [
      { id: 'model', name: 'Model', type: 'select', currentValue: current, options: groups },
      ...(current === flashModel ? [{
        id: 'reasoning_effort', name: 'Reasoning effort', type: 'select', currentValue: currentEffort,
        options: [{ value: '', name: 'Provider default' }, { value: 'high', name: 'High' }],
      }] : []),
    ];
    const orderPreferencesKey = 'kwork-test-session-order';
    const modelTests = ['model-menu', 'model-catalog', 'model-accessibility'];
    const secondTeamSession = 'team-second-session';
    const teamModes = new Map([[sessionId, false], [secondTeamSession, true]]);
    window.__verification = { calls, events: [], emit: payload => listeners.forEach(listener => listener({ event: 'dsh-gpui-event', payload })) };
    const stagedUploads = new Map();
    let nextUpload = 1;
    window.__verification.failFileChunk = false;
    const inbox = [];
    let nextInboxItem = 1;
    const changedInbox = () => window.__verification.emit({
      kind: 'session_update', sessionId, update: { sessionUpdate: 'gpui_inbox_changed' },
    });
    window.__verification.addInboxFixtures = () => {
      inbox.push({
        itemId: 'queued-image', mode: 'queue', text: 'Queued picture', editText: null, hasImage: true,
        attachments: [{ type: 'image', attachmentId, mediaType: 'image/png' }],
      }, {
        itemId: 'queued-file', mode: 'queue', text: 'Queued document', editText: null,
        attachments: [{ type: 'file', attachmentId: 'b'.repeat(64), name: 'fixture-report.pdf', bytes: 2304 }],
      });
      changedInbox();
    };
    window.__verification.admitInboxPicture = () => {
      const index = inbox.findIndex(row => row.itemId === 'queued-image');
      if (index >= 0) inbox.splice(index, 1);
      window.__verification.emit({ kind: 'session_update', sessionId, update: {
        sessionUpdate: 'gpui_user_message', messageId: 'queued-image', content: 'Queued picture',
        images: [{ attachmentId, mediaType: 'image/png', name: 'fixture.png' }], hasImage: true,
      } });
      changedInbox();
    };
    window.__verification.requestInboxSnapshot = () => {
      window.__verification.holdInboxSnapshot = true;
      changedInbox();
    };
    window.__verification.updateModelCatalog = scenario => {
      const configOptions = scenario === 'trimmed'
        ? modelConfig({ groups: [{ ...customGroup, options: [customGroup.options[1]] }, officialGroup ] })
        : scenario === 'unselected' ? modelConfig({ current: '' }) : modelConfig();
      window.__verification.emit({
        kind: 'session_update', sessionId,
        update: { sessionUpdate: 'config_option_update', configOptions },
      });
    };
    window.__verification.finishSearch = (query, items, hasMore) => searchRequests.get(query)?.shift()?.({ items, hasMore });
    for (const phase of ['pointerdown', 'pointerup', 'click']) {
      window.addEventListener(phase, event => window.__verification.events.push({ phase, x: event.clientX, y: event.clientY, hit: document.elementFromPoint(event.clientX, event.clientY)?.tagName }), true);
    }
    const initialAccount = {
      available: true, platformEmbedded: accountScenario !== 'legacy', modelReady: restoredAccount, state: {
        status: restoredAccount ? 'credential-stored' : 'signed-out', attempt: null,
        links: { usageUrl: 'https://fixture.invalid/usage', topUpUrl: 'https://fixture.invalid/top_up' },
      },
      ...(mode === 'account-notifications' ? { notifications: { scope: 'first-grant', mode: 'modern', pollIntervalMs: 600000 } } : {}),
    };
    window.__verification.accountState = initialAccount;
    if (mode === 'account-notifications') {
      window.__verification.notificationVisible = noticeIntro;
      Object.defineProperty(document, 'visibilityState', { configurable: true,
        get: () => window.__verification.notificationVisible ? 'visible' : 'hidden' });
    }
    window.__TAURI__ = {
      core: { invoke: async (name, args = {}) => {
        args = JSON.parse(JSON.stringify(args, (_, value) => value instanceof Map ? Object.fromEntries(value) : value));
        calls.push({ name, args });
        switch (name) {
          case 'start_auth': case 'auth_status': {
            if (['desktop-onboarding', 'account-notifications'].includes(mode)) return { apiKey: { configured: onboardingScenario === 'api-key' } };
            if (['provider-settings', 'provider-protocols', 'provider-authorization', 'deepseek-account'].includes(mode)) return { apiKey: { configured: false } };
            if (mode !== 'welcome' || welcomeCase === 'configured') return { apiKey: { configured: true } };
            return new Promise(resolve => {
              window.__verification.settleWelcomeAuth = () => resolve({ apiKey: { configured: false } });
            });
          }
          case 'save_api_key': return new Promise((resolve, reject) => {
            window.__verification.settleWelcomeSave = failure => {
              delete window.__verification.settleWelcomeSave;
              if (failure) reject(failure);
              else resolve({ apiKey: { configured: true } });
            };
          });
          case 'models_settings': return window.__modelsRemote ? window.__modelsRemote(args.request) : { ok: false, error: { code: 'fixture/unavailable', message: 'This fixture does not supply model editing' } };
          case 'model_providers': {
            if (!['provider-settings', 'provider-protocols', 'provider-authorization'].includes(mode)) return args.action === 'list' ? [] : null;
            if (mode === 'provider-authorization' && args.action === 'authorize' && args.draft.action === 'status') {
              return window.__verification.providerLoginState;
            }
            return new Promise((resolve, reject) => {
              window.__verification.settleProviderSettings = (answer, error) => {
                delete window.__verification.settleProviderSettings;
                if (error) reject(error);
                else resolve(answer);
              };
            });
          }
          case 'open_browser_url': return args.url;
          case 'browser_shell_generation': return ['deepseek-account', 'desktop-onboarding', 'account-quota'].includes(mode) && accountScenario !== 'browser' ? 11 : null;
          case 'desktop_onboarding': {
            if (mode !== 'desktop-onboarding') return { snapshot: onboardingDone, conflict: false };
            if (args.action.action === 'read') return { snapshot: onboardingSnapshot, conflict: false };
            return new Promise((resolve, reject) => {
              window.__verification.settleOnboarding = (next, error) => {
                delete window.__verification.settleOnboarding;
                if (error) { reject(error); return; }
                onboardingSnapshot = next;
                if (next.progress.step === 'done' && window.__clientPreferenceState) {
                  const row = window.__clientPreferenceState.namespaces.find(row => row.ns === 'ui-settings');
                  row.value.enabled = next.progress.developerTools; row.revision++;
                  sessionStorage.setItem('fixture-client-preferences', JSON.stringify(window.__clientPreferenceState));
                  window.__verification.emit({kind:'client_preferences_changed'});
                }
                sessionStorage.setItem(onboardingKey, JSON.stringify(next));
                resolve({ snapshot: next, conflict: false });
              };
            });
          }
          case 'account_platform_view': {
            if (args.shell !== 11) throw new Error('Platform request used the wrong shell');
            const pending = window.__verification.platformRequests ??= {};
            if (args.request.action === 'open'
                || (args.request.action === 'close' && window.__verification.holdPlatformClose)) {
              return new Promise((resolve, reject) => {
                pending[`${args.request.action}:${args.request.id}`] = { resolve, reject };
              });
            }
            return null;
          }
          case 'account_notifications': {
            if (args.request.action === 'report') return true;
            const pending = window.__verification.noticeRequests ??= [];
            return new Promise(resolve => pending.push({ request: args.request, resolve }));
          }
          case 'deepseek_account': {
            if (args.request.action === 'status') return window.__verification.accountState;
            const queues = window.__verification.accountRequests ??= {};
            const queue = queues[args.request.action] ??= [];
            return new Promise((resolve, reject) => {
              queue.push({ resolve, reject });
            });
          }
          case 'load_workspace_preferences': {
            const defaults = { themeMode: theme ?? (mode === 'appearance' ? 'light' : 'dark'), addedWorkspaces: [cwd], workDetails: 'verbose', contentFontDelta: 0, scratchSession: sessionId };
            if (['deepseek-account', 'desktop-onboarding', 'account-notifications', 'account-quota'].includes(mode)) return { ...defaults,
              ...(mode === 'desktop-onboarding' && onboardingSnapshot.progress.step === 'done'
                ? { workDetails: onboardingSnapshot.progress.process, performanceUsage: onboardingSnapshot.progress.usage } : {}),
              onboarding: onboardingSnapshot };
            if (startup) return { themeMode: 'dark', selectedSession: startup === 'restore' ? sessionId : null };
            if (workspaceRegistry) return { ...defaults,
              workspaceOrder: [cwd, ...hostWorkspaces.filter(item => item.path !== cwd).map(item => item.path)],
              groupBy: 'workspaceTree', pinned: mode === 'workspace-search' ? ['index-suffix'] : [outsiderId],
              ...(mode === 'workspace-search' ? { orderBy: 'manual' } : {}),
              ...(mode === 'workspace-actions' ? { aliases: { [cwd]: 'Old local alias' }, hidden: [cwd] } : {}),
            };
            if (['appearance', 'fonts'].includes(mode)) return JSON.parse(sessionStorage.getItem('kwork-test-appearance')) ?? defaults;
            if (mode !== 'session-order') return defaults;
            return JSON.parse(sessionStorage.getItem(orderPreferencesKey)) ?? {
              ...defaults, addedWorkspaces: [cwd, orderWorkspace], workspaceOrder: [cwd, orderWorkspace],
              collapsed: [orderWorkspace], pinned: ['pin-beta', 'pin-alpha'], archived: ['archived'],
              groupBy: 'workspace', orderBy: 'updated',
            };
          }
          case 'load_shortcuts': return { revision: 1, document: { schemaVersion: 1, profiles: {} }, error: null };
          case 'load_developer_tools': return ['ui-feedback', 'file-links', 'chat-markdown'].includes(mode) ? false : mode === 'desktop-onboarding' ? onboardingSnapshot.progress.developerTools : window.__developerTest?.enabled ?? true;
          case 'save_developer_tools': return args.enabled;
          case 'client_preferences': {
            if (!window.__clientPreferenceState) {
              const prefs = await window.__TAURI__.core.invoke('load_workspace_preferences', {});
              window.__clientPreferenceState = JSON.parse(sessionStorage.getItem('fixture-client-preferences')) ?? { writable: true, namespaces: [
                {ns:'ui-theme',revision:0,value:{preference:prefs.themeMode ?? 'system',fontSize:14+(prefs.contentFontDelta ?? 0),
                  codeFontSize:prefs.codeFontSize ?? 11,terminalFontSize:prefs.terminalFontSize ?? 13,textFontFamily:prefs.textFontFamily ?? '',
                  codeFontFamily:prefs.codeFontFamily ?? '',terminalFontFamily:prefs.terminalFontFamily ?? ''}},
                {ns:'ui-settings',revision:0,value:{enabled:['ui-feedback', 'file-links', 'chat-markdown'].includes(mode) ? false : mode === 'desktop-onboarding' ? onboardingSnapshot.progress.developerTools : window.__developerTest?.enabled ?? true}},
              ]};
            }
            const state = window.__clientPreferenceState, request = args.request;
            if (request.operation === 'set') {
              if (mode === 'fonts' && window.__fontSaveFailure > 0) { window.__fontSaveFailure--; throw Error('fixture refused font save'); }
              const row=state.namespaces.find(row=>row.ns===request.namespace);
              if (!state.writable || row.revision!==request.revision) throw Error('SETTINGS_CONFLICT');
              if (request.namespace==='ui-settings' && window.__developerRemote) await window.__developerRemote(request.changes.enabled);
              Object.assign(row.value,request.changes);row.revision++;
              sessionStorage.setItem('fixture-client-preferences',JSON.stringify(state));
              window.__verification.emit({kind:'client_preferences_changed'});
            }
            return structuredClone(state);
          }
          case 'start_service': return { workspace: cwd, servicePath: '/fixture', agent: { agentInfo: { name: 'fixture' } }, configOptions: [{ id: 'model', currentValue: 'fixture-model' }], sessionReading: { available: mode !== 'workspace-accessibility', items: [] },
            ...(['deepseek-account', 'desktop-onboarding', 'account-notifications', 'account-quota'].includes(mode) ? { deepseekAccount: initialAccount } : {}),
            ...(welcomeCase === 'compatible' ? { modelProviders: [{ id: 'native-compat', available: true, keyConfigured: true, name: 'Existing provider', baseUrl: 'https://fixture.invalid/v1', protocol: 'openai-completions', models: [{ id: 'fixture-model' }] }] } : {}),
            ...(workspaceRegistry ? { workspaceRegistry: { available: true, sequence: 1, items: hostWorkspaces, pinned: mode === 'workspace-search' ? ['index-suffix'] : [],
              archived: mode === 'workspace-search' ? ['archived-match'] : [] } } : {}),
            ...(startup ? { workspaceRegistry: { available: true, sequence: 1, items: startupWorkspaces, pinned: [], archived: [] } } : {}),
          };
          case 'list_sessions': {
            if (startup === 'recent') return { sessions: startupSessions.map(({ sessionId, cwd }) => ({ sessionId, cwd })) };
            return { sessions: startup ? startupSessions : mode === 'session-order' ? orderSessions : mode === 'workspace-search' ? searchSessions : [
              { sessionId, cwd, title: '公式与工具结果验收', createdAt: 1 },
              ...(['ui-feedback', 'chat-markdown'].includes(mode) ? [{ sessionId: 'feedback-cold', cwd, title: '加载中的历史会话', blank: false, createdAt: 2 }] : []),
              ...(mode === 'team' ? [{ sessionId: secondTeamSession, cwd, title: '使用过模型的团队会话', createdAt: 2 }] : []),
              ...(workspaceRegistry ? [{ sessionId: outsiderId, cwd, title: mode === 'workspace-actions' ? 'Other workspace session' : 'Ungrouped despite same cwd', createdAt: 2 }] : []),
            ], nextCursor: null };
          }
          case 'search_session_history': {
            if (mode !== 'workspace-search' || args.query == null) return { items: [], hasMore: false };
            return new Promise(resolve => {
              const queue = searchRequests.get(args.query) ?? [];
              queue.push(resolve);
              searchRequests.set(args.query, queue);
            });
          }
          case 'list_local_sessions': {
            if (mode === 'workspace-search') return { sessions: [
              { ...searchSessions[1], lastPromptAt: 3000 },
              searchSessions.find(session => session.blank),
            ] };
            if (startup === 'recent') return { sessions: [{ ...startupSessions[0], lastPromptAt: 3000 }, startupSessions[1]] };
            if (mode === 'session-order') return { sessions: [{ ...orderSessions.find(session => session.sessionId === 'first'), lastPromptAt: 800 }] };
            return { sessions: [] };
          }
          case 'peek_session_titles': return mode === 'session-order' ? {} : { [sessionId]: '公式与工具结果验收' };
          case 'new_session':
            if (mode === 'agent-presets' && args.agentPreset === 'cordis') {
              window.__presetTest.sessions['preset-creator'] = 'cordis';
              return {sessionId:'preset-creator', cwd, hostRunning:false, agentPreset:'cordis',
                configOptions:[{id:'model',currentValue:'fixture-model'}]};
            }
            // Fall through to the common session reply.
          case 'resume_session': return {
            sessionId: workspaceRegistry || startup || ['team', 'ui-feedback'].includes(mode) ? args.sessionId ?? (mode === 'team' ? sessionId : 'picked-session') : sessionId,
            cwd: args.cwd ?? cwd, configOptions: modelTests.includes(mode) ? modelConfig() : [
              { id: 'model', currentValue: 'fixture-model' },
              ...(mode === 'team' ? [{ id: 'reasoning_effort', currentValue: 'high', options: [
                { value: 'low', name: 'Low' }, { value: 'high', name: 'High' },
              ] }] : []),
            ], hostRunning: false,
            ...(startup === 'fresh' ? { workspaceRegistry: { available: true, sequence: 3,
              items: [{ ...defaultWorkspace, sessionIds: ['picked-session'] }], archived: [], pinned: [] } } : {}),
          };
          case 'model_catalog': {
            if (mode !== 'model-catalog' && mode !== 'model-accessibility') return { available: false };
            return new Promise((resolve, reject) => {
              window.__verification.settleCatalog = scenario => {
                delete window.__verification.settleCatalog;
                if (scenario === 'error') return reject('Fixture catalog transport failed');
                const extendedOfficial = { ...officialGroup, options: [
                  ...officialGroup.options,
                  { value: route('deepseek-official', 'offline-balanced'), name: 'Balanced' },
                  { value: route('deepseek-official', 'offline-general'), name: 'General' },
                  { value: route('deepseek-official', 'offline-specialized'), name: 'Specialized' },
                ] };
                if (scenario === 'stale') {
                  resolve({ available: true, groups: [{ ...customGroup, options: [
                    { value: route('custom-host', 'ghost'), name: 'Ghost model' },
                  ] }], failures: [] });
                } else {
                  resolve({ available: true,
                    groups: scenario === 'partial' ? [extendedOfficial] : [customGroup, extendedOfficial],
                    failures: scenario === 'partial'
                      ? [{ id: customGroup.group, name: customGroup.name, message: 'Fixture provider timed out' }] : [],
                  });
                }
              };
            });
          }
          case 'set_config_option': {
            if (!modelTests.includes(mode)) return null;
            return new Promise((resolve, reject) => {
              window.__verification.settleModelChoice = failure => {
                delete window.__verification.settleModelChoice;
                if (failure) return reject(failure);
                if (args.configId === 'model') {
                  currentModel = args.value;
                  currentEffort = '';
                } else {
                  currentEffort = args.value;
                }
                resolve({ configOptions: modelConfig() });
              };
            });
          }
          case 'pick_workspace_directory': return chosenDirectory;
          case 'open_start_tool': return null;
          case 'workspace_applications': return mode === 'workspace-accessibility'
            ? [{ id: 'fixture-editor', name: 'Fixture editor' }]
            : ['trajectory-folds', 'trajectory-menus'].includes(mode) ? [{ id: 'finder', name: '访达' }, { id: 'terminal', name: '终端' }] : [];
          case 'rename_session': return new Promise((resolve, reject) => {
            window.__verification.settleSessionRename = result => result.error ? reject(result.error) : resolve(result);
          });
          case 'set_session_archived': {
            if (mode === 'workspace-search') return { available: true, sequence: 4, items: hostWorkspaces, archived: [] };
            if (mode !== 'workspace-accessibility') return null;
            if (!args.stopActivity) return { activity: [{ kind: 'turn' }] };
            return { available: true, sequence: 4, items: hostWorkspaces, pinned: [], archived: [args.sessionId] };
          }
          case 'workspace_registry': {
            if (args.operation.action === 'initialize-default') return new Promise((resolve, reject) => {
              window.__verification.settleDefaultWorkspace = result => result.error ? reject(result.error) : resolve(result);
            });
            if (args.operation.action === 'create') return { available: true, sequence: 10, path: canonicalDirectory,
              items: [{ id: 'host-picked', path: canonicalDirectory, title: 'Canonical picker', sessionIds: [] }, ...hostWorkspaces], pinned: [], archived: [] };
            if (['pin', 'unpin'].includes(args.operation.action)) return new Promise((resolve, reject) => {
              window.__verification.settleWorkspacePin = result => result.error ? reject(result.error) : resolve(result);
            });
            if (['rename', 'delete'].includes(args.operation.action)) return new Promise((resolve, reject) => {
              window.__verification.settleWorkspaceAction = result => result.error ? reject(result.error) : resolve(result);
            });
            return new Promise((resolve, reject) => {
              window.__verification.settleWorkspaceMove = result => result.error ? reject(result.error) : resolve(result);
            });
          }
          case 'resolve_session_document':
            return {path: args.path.startsWith('/') ? args.path : cwd + '/' + args.path,cwd};
          case 'document_file': {
            if (args.path.endsWith('missing.png')) throw new Error('not found');
            return {absolutePath:args.path,size:12,...args.read ? {data:[...Uint8Array.from(atob(png),c=>c.charCodeAt(0))]} : {}};
          }
          case 'document_applications': return [{id:'/Applications/Preview.app',name:'预览',default:true}];
          case 'open_document_application': case 'open_url': return null;
          case 'save_file_bytes': return {status:args.mode==='saveAs'?'cancelled':'saved',path:'/fixture/downloads/'+args.name};
          case 'read_workspace_text': {
            if (mode === 'document-question') {
              const offset = args.offset;
              const absolutePath = cwd + '/docs/guide.md';
              const version = 'guide-1';
              if (offset === 1) return {absolutePath, version, offset, eof: false, lines: 9,
                text:'---\n标题: 示例\n版本: 1.0\n---\n# 分页文档\n\n![文件旁图片](preview%20image.png?version=1)\n\n第一段'};
              if (!window.__verification.retryDocument) {
                window.__verification.retryDocument = true;
                throw Error('Fixture page failed');
              }
              return {absolutePath,version,offset,eof:true,lines:3,text:'\n文档最后一段 $E=mc^2$。\n'};
            }
            const total = 120;
            const count = Math.min(40, total - args.offset + 1);
            return {absolutePath:cwd + '/' + args.path, version:'lines-v1', offset:args.offset, lines:count,
              eof:args.offset + count > total,
              text:Array.from({length:count},(_,i) => `const line_${args.offset+i} = "第 ${args.offset+i} 行";`).join('\n')};
          }
          case 'read_workspace_html': return { absolutePath: cwd + '/' + args.path, version: 'fixture-v1',
            source: '<!doctype html><meta name="viewport" content="width=device-width,initial-scale=1"><h1>完整预览</h1><p>右侧预览内容</p>' };
          case 'respond_question': case 'respond_plan_review': {
            const isPlan = name === 'respond_plan_review';
            if (!window.__verification[name + "SucceededOnRetry"]) {
              window.__verification[name + "SucceededOnRetry"] = true;
              throw Error('验收夹具拒绝首次提交');
            }
            window.__verification.emit({kind:'session_update', sessionId:args.sessionId,
              update:{sessionUpdate:isPlan ? 'gpui_plan_review' : 'gpui_questions', phase:'closed', requestId:args.requestId}});
            return null;
          }
          case 'read_session_plans': return new Promise((resolve, reject) => {
            window.__verification.settlePlan = reply => reply.error ? reject(reply.error) : resolve(reply);
          });
          case 'load_session_history': {
            if (['ui-feedback', 'chat-markdown'].includes(mode) && args.sessionId === 'feedback-cold') {
              return new Promise(resolve => { window.__verification.finishColdHistory = () => resolve(history); });
            }
            if (mode === 'trajectory-pages' && args.beforeSeq != null) {
              return new Promise((resolve, reject) => {
                window.__verification.settleOlderPage = reply => reply.error ? reject(reply.error) : resolve(reply);
              });
            }
            return history;
          }
          case 'turn_token_usage': return {};
          case 'prompt': return new Promise(resolve => {
            window.__verification.completePrompt = () => resolve({ stopReason: 'end_turn' });
          });
          case 'respond_permission': return new Promise((resolve, reject) => {
            window.__verification.settleApproval = failure => {
              delete window.__verification.settleApproval;
              if (failure) reject(failure);
              else resolve(null);
            };
          });
          case 'permission_mode': {
            if (mode !== 'permission') return { mode: 'default' };
            return new Promise((resolve, reject) => {
              window.__verification.settlePermission = failure => {
                delete window.__verification.settlePermission;
                if (failure) return reject(failure);
                if (args.preset) currentPermission = args.preset;
                resolve({ currentValue: currentPermission, options: permissionOptions });
              };
            });
          }
          case 'plan_mode': return { active: false };
          case 'question_state': return [];
          case 'session_commands': return mode === 'attachments' ? [
            { name: 'compact' }, { name: 'plan', inputHint: '[off|message]', attachments: true },
            { name: 'goal', inputHint: '<objective>', attachments: true },
          ] : [];
          case 'inbox_state': {
            const snapshot = { requestSeq: args.requestSeq, items: structuredClone(inbox) };
            if (!window.__verification.holdInboxSnapshot) return snapshot;
            delete window.__verification.holdInboxSnapshot;
            return new Promise(resolve => {
              window.__verification.releaseInboxSnapshot = () => resolve(snapshot);
            });
          }
          case 'composer_preference': {
            const current = () => sessionStorage.getItem('fixture-composer-mode') ?? 'queue';
            if (!args.mode) return { mode: current() };
            return new Promise((resolve, reject) => {
              window.__verification.settleComposer = error => {
                delete window.__verification.settleComposer;
                if (error) reject(error);
                else {
                  sessionStorage.setItem('fixture-composer-mode', args.mode);
                  resolve({ mode: current() });
                }
              };
            });
          }
          case 'stage_prompt_file': {
            const operation = args.operation;
            if (operation.kind === 'begin') {
              const id = `uploaded-${nextUpload++}`;
              stagedUploads.set(id, { name: operation.name, size: operation.size, offset: 0 });
              return { id };
            }
            if (operation.kind === 'discard') {
              stagedUploads.delete(operation.id);
              return null;
            }
            const upload = stagedUploads.get(operation.id);
            if (!upload || upload.offset !== operation.offset || operation.offset + operation.chunk.length > upload.size) {
              throw 'Invalid fixture upload offset';
            }
            if (window.__verification.failFileChunk && operation.offset > 0) {
              window.__verification.failFileChunk = false;
              throw 'Fixture interrupted the attachment upload';
            }
            upload.offset += operation.chunk.length;
            return null;
          }
          case 'download_user_attachment': return { saved: true, name: args.name };
          case 'execute_session_command': {
            for (const block of args.attachments) {
              if (block.type !== 'file') continue;
              const file = stagedUploads.get(block.uploadId);
              if (!file || file.offset !== file.size) throw 'Fixture command missing file upload';
            }
            if (args.line === '/plan off' && args.attachments.length > 0) throw 'Attachments cannot accompany /plan off';
            return { kind: 'success', planActive: args.line.startsWith('/plan ') };
          }
          case 'queue_prompt': return new Promise((resolve, reject) => {
            const text = args.prompt.map(block => block.type === 'text' ? block.text : '').join('');
            const attachments = args.prompt.flatMap(block => {
              if (block.type !== 'file') return [];
              const file = stagedUploads.get(block.uploadId);
              if (!file || file.offset !== file.size) throw 'Fixture missing a complete upload';
              const digest = Number(block.uploadId.split('-').at(-1)).toString(16).padStart(64, '0');
              return [{ type: 'file', attachmentId: 'sha256:' + digest, name: file.name, bytes: file.size }];
            });
            const item = { itemId: `queue-${nextInboxItem++}`, mode: args.mode, text,
              editText: attachments.length ? null : text, attachments,
              requestId: args.requestId, hasImage: false };
            window.__verification.acceptQueue = () => {
              inbox.push(item);
              changedInbox();
            };
            window.__verification.admitQueue = () => {
              const index = inbox.indexOf(item);
              if (index >= 0) inbox.splice(index, 1);
              window.__verification.admitted = item.itemId;
              window.__verification.emit({ kind: 'session_update', sessionId, update: {
                sessionUpdate: 'gpui_user_message', messageId: item.itemId,
                requestId: item.requestId, content: item.text, files: item.attachments,
              } });
              changedInbox();
            };
            window.__verification.settleQueue = error => {
              delete window.__verification.settleQueue;
              if (error) return reject(error);
              if (!inbox.includes(item) && window.__verification.admitted !== item.itemId) {
                inbox.push(item);
                changedInbox();
              }
              resolve(item);
            };
          });
          case 'inbox_action': return new Promise((resolve, reject) => {
            window.__verification.settleInboxAction = error => {
              delete window.__verification.settleInboxAction;
              if (error) return reject(error);
              const index = inbox.findIndex(item => item.itemId === args.itemId);
              if (index < 0) return reject('Fixture missing item');
              const item = inbox[index];
              if (args.action === 'edit') Object.assign(item, { text: args.text, editText: args.text });
              if (args.action === 'remove') inbox.splice(index, 1);
              if (args.action === 'steer') item.mode = 'steer';
              changedInbox();
              resolve({ item });
            };
          });
          case 'cancel_session': return null;
          case 'goal_state': return mode === 'goal' ? { goal, sequence: 1 } : { goal: null, sequence: 1 };
          case 'goal_action': return new Promise((resolve, reject) => {
            window.__verification.settleGoal = (error, snapshot) => error ? reject(error) : resolve(snapshot);
          });
          case 'session_read_state': {
            window.__verification.emit({ kind: 'session_read', sessionId, completedTurn: 1, failed: true, readTurn: args.action.throughTurn, manualUnread: false });
            return null;
          }
          case 'team_state': {
            const read = () => ({ available: true, request: args.request, team: {
              enabled: teamModes.get(args.sessionId) ?? false,
              hasUsedModel: args.sessionId === secondTeamSession,
              tasks: [{ id: 'task-api', subject: '实现 API', description: '补齐接口和测试', status: 'in_progress', ready: true, ownerId: 'child-api', ownerName: 'API Agent' }],
            } });
            if (typeof args.enabled !== 'boolean') return read();
            const save = () => {
              teamModes.set(args.sessionId, args.enabled);
              return read();
            };
            if (args.sessionId !== secondTeamSession && !window.__verification.deferTeamSave) return save();
            return new Promise((resolve, reject) => {
              const replies = window.__verification.teamReplies ??= {};
              replies[args.sessionId] = failure => {
                delete replies[args.sessionId];
                if (failure) reject(failure);
                else resolve(save());
              };
            });
          }
          case 'settings_document': {
            if (mode !== 'settings-shell') return { available: false };
            const requests = window.__verification.documentRequests ??= [];
            return new Promise((resolve, reject) => requests.push({ operation: args.operation, resolve, reject }));
          }
          case 'agent_preset_settings': return window.__presetRemote ? window.__presetRemote(args.request) : { presets: [], writable: false };
          case 'plugin_inventory': return window.__inventoryRemote ? window.__inventoryRemote() : { entries: [] };
          case 'permission_default': return window.__permissionRemote ? window.__permissionRemote(args.request) : { view: { writable: false, namespaces: [] }, catalog: null };
          case 'session_log_preference': return window.__uploadRemote ? window.__uploadRemote(args.request) : { available: false };
          case 'web_settings': {
            const snapshot = window.__verification.webSettingsSnapshot ||= {
              search: { value: 'deepseek-search-api', base: null, options: [
                { id: 'deepseek-search-api', title: 'DeepSeek', description: '使用 DeepSeek 搜索 API。' },
                { id: 'deepseek-official', title: 'DeepSeek（旧版）' },
              ] },
              fetch: { value: 'http', base: null, options: [{ id: 'http', title: '本机直接获取' }, { id: 'deepseek-fetch-api', title: 'DeepSeek Fetch API' }] },
              fields: { count: { value: 10, base: 10, overridden: false } },
              credential: null,
              searchBilling: { value: 'auto', advancedOverride: [] },
              fetchBilling: { value: 'auto', advancedOverride: [] },
              revisions: { web: 1, 'web-deepseek-api': 1 }, writable: mode !== 'settings-readonly', providersFailed: false,
            };
            if (args.change) {
              const applied = [];
              for (const capability of ['search', 'fetch']) {
                if (args.change[capability] !== undefined) {
                  if (mode === 'settings-partial' && capability === 'fetch') continue;
                  snapshot[capability].value = args.change[capability];
                  applied.push(capability);
                }
                const billingKey = `${capability}Billing`;
                if (args.change[billingKey] !== undefined) {
                  snapshot[billingKey].value = args.change[billingKey];
                  applied.push(billingKey);
                }
              }
              for (const [key, edit] of Object.entries(args.change.fields || {})) {
                const field = snapshot.fields[key];
                if (!field) continue;
                if (edit.op === 'unset') { field.value = field.base; field.overridden = false; }
                else { field.value = edit.value; field.overridden = true; }
                applied.push(key);
              }
              snapshot.revisions.web++;
              if (applied.some(key => key.endsWith('Billing'))) snapshot.revisions['web-deepseek-api']++;
              return { snapshot, applied, ...(mode === 'settings-partial' ? { failure: '网页获取提供方未保存，未保存的内容已保留。' } : {}) };
            }
            return { snapshot, applied: [] };
          }
          case 'plugin_center': return {
            configs: ['webSearch'], managementAvailable: true,
            entries: [
              { entryId: 'active', moduleName: '@fixture/active', enabled: true, fiberPhase: 'active' },
              { entryId: 'off', moduleName: '@fixture/off', enabled: false, fiberPhase: 'active' },
              { entryId: 'failed', moduleName: '@fixture/failed', enabled: true, fiberPhase: 'failed', diagnostic: 'fixture failure' },
            ],
            bundles: [
              {
                name: '@fixture/terminal', version: '1.0.0', title: '终端增强',
                description: 'Agent 终端与沙箱扩展贡献。', installed: true, official: true,
                experimental: true, enabled: true, removable: false, availability: 'installed',
                catalogVersion: '1.1.0', catalogUpdate: true, updatable: false,
                moreOnly: false, rank: 0,
                rows: [
                  { rowId: 'active', moduleName: '@fixture/active', entryId: 'active', title: 'Agent 终端' },
                  { rowId: 'off', moduleName: '@fixture/off', entryId: 'off', title: '终端安装器' },
                  { rowId: 'failed', moduleName: '@fixture/failed', entryId: 'failed', title: '终端桥接' },
                ],
              },
              {
                name: '@fixture/not-running', version: '1.0.0', title: '未运行扩展',
                description: '当前平台没有启动该插件。', installed: true, official: false,
                experimental: false, enabled: true, removable: true, availability: 'installed',
                error: 'unsupported-platform', updatable: false, moreOnly: false, rank: 1, rows: [],
              },
            ],
          };
          case 'watch_tool_result': {
            window.__verification.toolResultWatch = args;
            if (args.sessionId != null) window.__verification.emit({ kind: 'tool_result', ...args, update: 1, available: false });
            return { active: args.sessionId != null };
          }
          case 'agent_terminal_watch': return [];
          case 'agent_terminal_attach': return {attached:true,frame:{type:'snapshot',sequence:1,screen:'font grid 0123456789\r\nsecond row\r\n$ ',writable:true,info:{
            id:'font-terminal',title:'Font grid',cwd:'/fixture',cols:100,rows:30,state:'running',exitCode:null,origin:'agent',control:'agent',sandboxMode:'workspace-write',shell:'zsh'
          }}};
          case 'agent_terminal_input': return {sequence:args.sequence};
          case 'agent_terminal_detach': case 'agent_terminal_close': return null;
          case 'read_image_file': return {path:args.path,mediaType:'image/png',dataUrl:`data:image/png;base64,${png}`};
          case 'read_attachment':
            if (mode === 'trajectory-blocks' && args.attachmentId === 'b'.repeat(64) && !window.__verification.imageAvailable) {
              throw new Error('Fixture image remains unavailable until recovery');
            }
            return { attachmentId:args.attachmentId, dataUrl: `data:image/png;base64,${png}` };
          case 'save_image_attachment': return {status:args.mode === 'saveAs' ? 'cancelled' : 'saved',path:'/fixture/downloads/'+args.name};
          case 'note_panic': console.error(`RUST PANIC ${args.message}`); return null;
          case 'save_workspace_preferences': {
            if (mode === 'session-order') sessionStorage.setItem(orderPreferencesKey, JSON.stringify(args.preferences));
            if (['appearance', 'fonts'].includes(mode)) sessionStorage.setItem('kwork-test-appearance', JSON.stringify(args.preferences));
            return null;
          }
          case 'configure_shell_window': case 'session_read_ack':
          case 'document_system_font': case 'chinese_system_font': case 'named_system_font': case 'list_shells': return [];
          default: console.log(`UNHANDLED_IPC ${name} ${JSON.stringify(args)}`); return null;
        }
      } },
      event: { listen: (name, listener) => { listeners.push(listener); return Promise.resolve(() => {}); } },
    };
    window.__TAURI_INTERNALS__ = { invoke: window.__TAURI__.core.invoke };
  }, { sessionId, cwd, history, png, attachmentId: imageAttachment.attachmentId, mode, orderWorkspace, hostWorkspaces, outsiderId, chosenDirectory, canonicalDirectory, defaultWorkspace, theme: process.env.KWORK_TEST_THEME });
  const url = `http://127.0.0.1:${server.address().port}/`;
  await page.goto(mode === 'workspace-startup' ? `${url}?scenario=fresh` : url);
  await page.waitForFunction(() => document.querySelector('canvas') && !document.getElementById('boot'), null, { timeout: 60000 });
  const clickConversationTab = async legacyX => {
    const sidebar = page.viewportSize().width < 1024 ? 0 : 280;
    const x = sidebar + 28 + 13 + (legacyX === 1271 ? 0 : 58);
    await page.mouse.move(x, 58);
    await page.waitForTimeout(150);
    await page.mouse.click(x, 58, { delay: 65 });
  };
  if (!['workspace-startup', 'welcome', 'provider-settings', 'provider-protocols', 'provider-authorization', 'deepseek-account', 'desktop-onboarding', 'account-notifications'].includes(mode)) {
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'load_session_history'), null, { timeout: 20000 }).catch(async error => {
      console.error('STARTUP_CALLS', JSON.stringify(await page.evaluate(() => window.__verification.calls)));
      await page.screenshot({ path: join(artifacts, 'startup-failed.png') });
      throw error;
    });
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'turn_token_usage'), null, { timeout: 20000 });
  }
  if (mode === 'document-question') {
    await runDocumentAndQuestionCase(page, artifacts, sessionId);
  } else if (mode === 'plan-preview') {
    await runPlanPreviewCase(page, artifacts, sessionId);
  } else if (mode === 'chat-markdown') {
    await runChatMarkdownCase(page, artifacts, sessionId);
  } else if (mode === 'file-links') {
    await page.screenshot({path:join(artifacts,'file-links-initial.png')});
    await page.locator('[data-chat-markdown]').getByRole('button', { name: '预览完整 HTML', exact: true }).click();
    await page.waitForFunction(() => window.__verification.calls.some(c => c.name === 'read_workspace_html' && c.args.path === '完整预览.html'));
    await page.locator('[data-document-html-host] iframe').waitFor();
    await page.screenshot({path:join(artifacts,'file-links-html.png')});
    await page.locator('[data-chat-markdown]').getByRole('button', { name: '跳到第 82 行', exact: true }).click();
    await page.waitForFunction(() => window.__verification.calls.some(c => c.name === 'read_workspace_text' && c.args.path === 'source.ts' && c.args.offset === 81));
    await page.locator('[data-document-html-host] iframe').waitFor({state:'detached'});
    await page.waitForTimeout(150);
    await page.screenshot({path:join(artifacts,'file-links-line-82-mark.png')});
    await page.waitForTimeout(2200);
    await page.screenshot({path:join(artifacts,'file-links-line-82.png')});
    const reads = await page.evaluate(() => window.__verification.calls.filter(c => c.name === 'read_workspace_text').length);
    await page.locator('[data-chat-markdown]').getByRole('button', { name: '回到第 3 行', exact: true }).click();
    await page.waitForFunction(() => window.__verification.calls.some(c => c.name === 'resolve_session_document' && c.args.path === './source.ts'));
    await page.waitForTimeout(150);
    await page.screenshot({path:join(artifacts,'file-links-line-3-mark.png')});
    assert.equal(await page.evaluate(() => window.__verification.calls.filter(c => c.name === 'read_workspace_text').length), reads, 'same file reuses its loaded document');
    await page.locator('[data-chat-markdown]').getByRole('button', { name: '工作区外文档', exact: true }).click();
    await page.waitForFunction(() => window.__verification.calls.some(c => c.name === 'read_workspace_text' && c.args.path === '../outside.txt'));
    await page.waitForTimeout(150);
    await page.screenshot({path:join(artifacts,'file-links-outside.png')});
    await page.locator('[data-chat-markdown]').getByRole('button', { name: '编码文件名', exact: true }).click();
    await page.waitForFunction(() => window.__verification.calls.some(c => c.name === 'read_workspace_text' && c.args.path === 'notes#draft?.txt'));
    const before = await page.evaluate(() => window.__verification.calls.filter(c => ['open_url','resolve_session_document'].includes(c.name)).length);
    await page.locator('[data-chat-markdown]').getByText('不能执行', {exact:true}).click();
    await page.waitForTimeout(100);
    assert.equal(await page.evaluate(() => window.__verification.calls.filter(c => ['open_url','resolve_session_document'].includes(c.name)).length), before, 'unsafe link remains inert');
    await writeFile(join(artifacts,'file-links-calls.json'),JSON.stringify(await page.evaluate(() => window.__verification.calls.filter(c => ['open_url','resolve_session_document','read_workspace_text','read_workspace_html'].includes(c.name))),null,2));
    assert.equal(errors.length,0);
  } else if (mode === 'ui-feedback') {
    await page.screenshot({ path: join(artifacts, 'chat-wide.png') });
    await page.getByRole('button', { name: '打开 加载中的历史会话', exact: true }).dispatchEvent('click');
    await page.waitForFunction(() => !!window.__verification.finishColdHistory);
    await page.getByRole('button', { name: '发送', exact: true }).waitFor({ state: 'hidden' });
    await page.screenshot({ path: join(artifacts, 'session-loading.png') });
    await page.evaluate(() => window.__verification.finishColdHistory());
    await page.getByRole('button', { name: '发送', exact: true }).waitFor({ state: 'visible' });
    await page.screenshot({ path: join(artifacts, 'session-loaded.png') });
    await page.setViewportSize({ width: 1024, height: 900 });
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(artifacts, 'chat-narrow.png') });
    await page.locator('[data-chat-markdown]').getByRole('button', { name: '打开 完整预览.html', exact: true }).click();
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'read_workspace_html'));
    await page.locator('[data-document-html-host] iframe').waitFor();
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(artifacts, 'chat-with-preview.png') });
    await page.setViewportSize({ width: 1440, height: 1100 });
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(artifacts, 'chat-with-preview-wide.png') });
    assert.equal(errors.length, 0);
  } else if (mode === 'fonts') {
    try { await runFontsCase(page, artifacts); }
    catch (error) { await page.screenshot({path:join(artifacts,'fonts-failed.png')}); await writeFile(join(artifacts,'fonts-failed.json'), JSON.stringify(await page.evaluate(() => ({html:document.querySelector('#settings-theme')?.outerHTML,calls:window.__verification.calls})),null,2)); throw error; }
  } else if (mode === 'settings-developer') {
    try { await runDeveloperCase(page, artifacts); }
    catch (error) {
      await page.screenshot({ path: join(artifacts, 'developer-failed.png') });
      throw error;
    }
  } else if (mode === 'settings-permission') {
    try { await runPermissionCase(page, artifacts); }
    catch (error) {
      await page.screenshot({ path: join(artifacts, 'permission-default-failed.png') });
      throw error;
    }
  } else if (mode === 'settings-upload') {
    try { await runUploadCase(page, artifacts); }
    catch (error) {
      await page.screenshot({ path: join(artifacts, 'upload-failed.png') });
      throw error;
    }
  } else if (mode === 'agent-presets') {
    try { await runAgentPresetsCase(page, artifacts); }
    catch (error) {
      await page.screenshot({path:join(artifacts,'presets-failed.png')});
      await writeFile(join(artifacts,'presets-failed.json'),JSON.stringify(await page.evaluate(() => ({text:document.body.innerText,calls:window.__presetTest?.calls})),null,2));
      throw error;
    }
  } else if (mode === 'plugin-inventory') {
    try { await runInventoryCase(page, artifacts); }
    catch (error) {
      await page.screenshot({ path: join(artifacts, 'inventory-failed.png') });
      await writeFile(join(artifacts, 'inventory-failed.txt'), await page.locator('body').innerText());
      throw error;
    }
  } else if (mode === 'models-official') {
    try { await runModelsCase(page, artifacts); }
    catch (error) {
      await page.screenshot({ path: join(artifacts, 'models-failed.png') });
      await writeFile(join(artifacts, 'models-failed.json'), JSON.stringify(await page.evaluate(() => ({ text: document.querySelector('#settings-models')?.innerText, state: window.__modelsTest })), null, 2));
      throw error;
    }
  } else if (mode === 'settings-shell') {
    page.setDefaultTimeout(12000);
    const shell = page.locator('#settings-shell');
    const nav = shell.getByRole('navigation');
    const open = shell.getByRole('button', { name: '打开配置文件', exact: true });
    const close = shell.getByRole('button', { name: '关闭设置', exact: true });
    const general = nav.getByRole('button', { name: '通用设置', exact: true });
    const requests = operation => page.waitForFunction(operation => window.__verification.documentRequests?.some(item => item.operation === operation), operation);
    const settle = (operation, result, failure = false) => page.evaluate(({ operation, result, failure }) => {
      const queue = window.__verification.documentRequests;
      const index = queue.findIndex(item => item.operation === operation);
      const [request] = queue.splice(index, 1);
      request[failure ? 'reject' : 'resolve'](result);
    }, { operation, result, failure });
    const frames = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    await page.locator('#composer-submit-accessibility').getByRole('button', { name: '更多设置', exact: true }).dispatchEvent('click');
    await page.locator('#composer-submit-accessibility').getByRole('button', { name: '设置', exact: true }).dispatchEvent('click');
    await shell.waitFor();
    await requests('describe');
    assert.equal(await open.count(), 0, 'document action remains hidden until Host metadata is ready');
    await settle('describe', { available: true });
    await open.waitFor();
    await frames();
    assert.equal(await nav.locator('[aria-current]').evaluate(node => node === document.activeElement), true, 'settings initially focuses the active page');
    assert.equal(await nav.locator('[aria-current]').evaluate(node => getComputedStyle(node).outlineStyle), 'none', 'automatic entry focuses without painting a keyboard ring');
    await page.keyboard.press('Tab');
    assert.equal(await nav.locator('[aria-current]').getAttribute('data-dsh-automatic-focus'), null, 'keyboard traversal restores normal focus presentation');
    assert.equal(await page.getByRole('button', { name: '通用设置', exact: true }).count(), 1, 'native settings navigation has no duplicate offscreen controls');
    assert.equal((await shell.boundingBox()).width, 800);
    assert.equal(await general.evaluate(node => getComputedStyle(node).color), 'rgb(249, 250, 251)', 'dark navigation uses the official primary-label token before any trajectory is rendered');
    assert.equal((await close.boundingBox()).height, 28);
    assert.equal((await open.boundingBox()).height, 28);
    const missingIcons = await shell.locator('use').evaluateAll(nodes => nodes.filter(node => !document.querySelector(node.getAttribute('href'))).map(node => node.getAttribute('href')));
    assert.deepEqual(missingIcons, []);
    await general.click();
    await page.waitForFunction(() => document.querySelector('#settings-shell [aria-current]')?.textContent === '通用设置');
    await open.click();
    await requests('open');
    assert.equal(await open.isDisabled(), true);
    await open.dispatchEvent('click');
    await frames();
    assert.equal(await page.evaluate(() => window.__verification.documentRequests.filter(item => item.operation === 'open').length), 1, 'repeated opening coalesces');
    await settle('open', 'private native path failure', true);
    await shell.getByRole('alert').waitFor();
    assert.equal(await shell.getByRole('alert').textContent(), '无法打开配置文件', 'native diagnostics do not leak into the user message');
    await open.click();
    await settle('open', { opened: true });
    await shell.getByRole('alert').waitFor({ state: 'hidden' });
    await nav.getByRole('button', { name: '模型', exact: true }).click();
    const models = page.locator('#settings-models');
    await models.getByRole('button', { name: '添加模型提供商', exact: true }).waitFor();
    assert.equal((await shell.boundingBox()).width, 800, 'model cards share the same panel width');
    assert.equal(await page.getByRole('button', { name: '关闭设置', exact: true }).count(), 1);
    const firstModelAction = models.getByRole('button', { name: '编辑 DeepSeek (deepseek-official)', exact: true });
    await close.focus();
    await page.keyboard.press('Tab');
    assert.equal(await firstModelAction.evaluate(node => node === document.activeElement), true, 'header Tab enters the official model cards');
    await page.keyboard.press('Shift+Tab');
    assert.equal(await close.evaluate(node => node === document.activeElement), true, 'model Shift+Tab returns to the shared header');
    await frames();
    await page.screenshot({ path: join(artifacts, 'providers-shared-header.png') });
    await writeFile(join(artifacts, 'settings-layering.json'), JSON.stringify(await page.evaluate(() => ({
      point: document.elementsFromPoint(1100, 530).map(node => ({ tag: node.tagName, id: node.id, class: node.className })),
      overlays: [...document.querySelectorAll('body *')].filter(node => ['fixed', 'absolute'].includes(getComputedStyle(node).position) && node.getClientRects().length && Number(getComputedStyle(node).zIndex) > 0).map(node => ({ tag: node.tagName, id: node.id, class: node.className, rect: node.getBoundingClientRect().toJSON() }))
    })), null, 2));
    await page.setViewportSize({ width: 620, height: 820 });
    await frames();
    assert.equal((await nav.boundingBox()).width, 188, 'Mac retains the desktop navigation at narrow widths');
    assert.equal((await shell.boundingBox()).width, 572);
    await page.screenshot({ path: join(artifacts, 'settings-narrow.png') });
    await general.click();
    await open.click();
    await requests('open');
    await page.evaluate(() => window.__verification.emit({ kind: 'service', status: 'exited' }));
    await open.waitFor({ state: 'hidden' });
    await page.evaluate(() => window.__verification.emit({ kind: 'service', status: 'running' }));
    await requests('describe');
    await settle('describe', { available: true });
    await open.waitFor();
    await settle('open', 'obsolete connection failure', true);
    await frames();
    assert.equal(await open.isEnabled(), true);
    assert.equal(await shell.getByRole('alert').count(), 0, 'obsolete opens do not affect the replacement connection');
    await close.focus();
    await page.keyboard.press('Escape');
    await shell.waitFor({ state: 'hidden' });
    await writeFile(join(artifacts, 'settings-document-calls.json'), JSON.stringify(await page.evaluate(() => window.__verification.calls.filter(call => call.name === 'settings_document')), null, 2));
  } else if (mode === 'account-quota') {
    page.setDefaultTimeout(16000);
    const dialog = page.getByRole('dialog', { name: '当前无可用余额', exact: true });
    const cancel = dialog.getByRole('button', { name: '取消', exact: true });
    const pay = dialog.getByRole('button', { name: '去充值', exact: true });
    const toast = page.locator('#account-quota-toast[role="alert"]');
    const platform = page.locator('#account-platform-view');
    const back = platform.getByRole('button', { name: '返回 KWork' });
    const account = page.locator('#deepseek-account-form');
    const focused = locator => locator.evaluate(node => node === document.activeElement);
    const frames = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    const fail = (turn, code) => page.evaluate(({ sessionId, turn, code }) => window.__verification.emit({
      kind: 'session_update', sessionId, update: { sessionUpdate: 'gpui_turn_tail', type: 'turn/end', turn, seq: turn * 10, reason: 'error',
        reasonObject: { kind: 'error', error: { code, message: 'Fixture quota failure' } } },
    }), { sessionId, turn, code });
    const opens = () => page.evaluate(() => window.__verification.calls
      .filter(call => call.name === 'account_platform_view' && call.args.request.action === 'open')
      .map(call => call.args.request.page));
    const loadPlatform = async () => {
      await page.waitForFunction(() => Object.keys(window.__verification.platformRequests ?? {}).some(key => key.startsWith('open:')));
      await page.evaluate(() => {
        const fixture = window.__verification;
        const key = Object.keys(fixture.platformRequests).find(key => key.startsWith('open:'));
        fixture.platformRequests[key].resolve(null);
        delete fixture.platformRequests[key];
        fixture.emit({ kind: 'account_platform_status', shell: 11, id: Number(key.split(':')[1]), status: 'loaded' });
      });
    };

    assert.equal(await dialog.count(), 0, 'loading a quota failure from history does not publish a notice');
    await fail(2, 'AUTH');
    await frames();
    assert.equal(await toast.count(), 0, 'only exact quota error codes publish a frame notice');
    await fail(3, 'QUOTA');
    await toast.waitFor();
    assert.equal((await toast.textContent()).trim(), '!当前请求的额度已用尽');
    await toast.waitFor({ state: 'detached' });
    await fail(4, 'ACCOUNT_QUOTA');
    await dialog.waitFor();
    assert.equal(await focused(cancel), true);
    await page.keyboard.press('Tab');
    assert.equal(await focused(pay), true);
    await page.keyboard.press('Tab');
    assert.equal(await focused(dialog.getByRole('button', { name: '关闭', exact: true })), true);
    await page.keyboard.press('Escape');
    await dialog.waitFor({ state: 'hidden' });
    await fail(4, 'ACCOUNT_QUOTA');
    await frames();
    assert.equal(await dialog.count(), 0, 'a repeated live row does not open another dialog');
    assert.deepEqual(await opens(), []);

    await page.locator('#deepseek-account-launcher > button').click();
    await page.getByRole('menuitem', { name: '设置', exact: true }).click();
    await page.locator('#settings-shell').getByRole('button', { name: '账号与余额', exact: true }).dispatchEvent('click');
    await account.getByRole('link', { name: '查询用量', exact: true }).click();
    await back.waitFor();
    await loadPlatform();
    await fail(5, 'ACCOUNT_QUOTA');
    await frames();
    assert.equal(await dialog.count(), 0);
    assert.equal(await focused(back), true);
    await back.click();
    await dialog.waitFor();
    assert.equal(await focused(cancel), true);
    await page.setViewportSize({ width: 760, height: 480 });
    await page.screenshot({ path: join(artifacts, 'account-quota-dialog.png') });
    await pay.click();
    await back.waitFor();
    await loadPlatform();
    await dialog.waitFor({ state: 'hidden' });
    await fail(6, 'ACCOUNT_QUOTA');
    await fail(7, 'QUOTA');
    await frames();
    assert.equal(await toast.count(), 0, 'later failures leave the owned top-up visit in place');
    assert.deepEqual(await opens(), ['usage', 'top-up']);
    await back.click();
    await platform.waitFor({ state: 'hidden' });
    assert.equal(await dialog.count(), 0);
    assert.equal(await toast.count(), 0);

    await fail(8, 'ACCOUNT_QUOTA');
    await dialog.waitFor();
    await pay.click();
    await back.waitFor();
    await loadPlatform();
    await page.evaluate(() => {
      const fixture = window.__verification;
      fixture.accountState.state.status = 'signed-out';
      fixture.emit({ kind: 'deepseek_account_changed', reason: 'changed' });
    });
    await platform.waitFor({ state: 'hidden' });
    await account.locator('[data-part="cards"]').waitFor({ state: 'hidden' });
    await fail(9, 'ACCOUNT_QUOTA');
    await toast.waitFor();
    assert.equal(await dialog.count(), 0);

    for (const scenario of ['legacy', 'browser']) {
      await page.goto(`${url}?scenario=${scenario}`);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'turn_token_usage'));
      await fail(10, 'ACCOUNT_QUOTA');
      await toast.waitFor();
      assert.equal(await dialog.count(), 0);
      assert.deepEqual(await opens(), []);
    }
    await page.goto(`${url}?scenario=intro`);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'turn_token_usage'));
    const onboardingTitle = page.getByRole('heading', { name: '欢迎使用 KWork', exact: true });
    await onboardingTitle.waitFor();
    await fail(10, 'ACCOUNT_QUOTA');
    await dialog.waitFor();
    assert.equal(await focused(cancel), true, 'quota dialog receives focus while onboarding holds the background');
    await cancel.click();
    await onboardingTitle.waitFor();
  } else if (mode === 'account-notifications') {
    page.setDefaultTimeout(16000);
    const stack = page.locator('#account-notifications');
    const front = stack.locator('[data-front="true"]');
    const account = page.locator('#deepseek-account-form');
    const button = (scope, name) => scope.getByRole('button', { name, exact: true });
    const make = (id, title, label, body = '测试通知正文') => ({ id, title, body, displayUntil: null,
      actions: [{ label, style: 'primary', target: 'account' },
        { label: '稍后处理', style: 'secondary', target: 'dismiss' }] });
    const A = make('notice-a', '赠金通知', '查看账号', '<img src=invalid> 纯文本');
    const B = make('notice-b', '余额提醒', '查看余额');
    const pending = (action, scope = 'first-grant', id = null) => page.waitForFunction(({ action, scope, id }) =>
      window.__verification.noticeRequests?.some(({ request }) =>
        request.action === action && request.scope === scope && (id === null || request.id === id)), { action, scope, id });
    const settle = (action, result, scope = 'first-grant', id = null) => page.evaluate(({ action, result, scope, id }) => {
      const queue = window.__verification.noticeRequests;
      const index = queue.findIndex(({ request }) =>
        request.action === action && request.scope === scope && (id === null || request.id === id));
      queue.splice(index, 1)[0].resolve(result);
    }, { action, result, scope, id });
    const show = (notices, scope) => settle('list', { nextPollAfterSeconds: 600, notifications: notices }, scope);
    const reports = () => page.evaluate(() => window.__verification.calls.filter(call => call.name === 'account_notifications'
      && call.args.request.action === 'report').map(call => [call.args.request.scope, call.args.request.id, call.args.request.event]));
    const event = (id, type) => page.waitForFunction(({ id, type }) => window.__verification.calls.some(call =>
      call.name === 'account_notifications' && call.args.request.id === id && call.args.request.event === type), { id, type });
    const visible = state => page.evaluate(value => {
      window.__verification.notificationVisible = value;
      document.dispatchEvent(new Event('visibilitychange'));
    }, state);

    await pending('list');
    await show([A, B]);
    await front.getByText(A.title, { exact: true }).waitFor();
    assert.deepEqual(await reports(), []);
    assert.equal(await front.locator('img').count(), 0);
    await front.getByText(A.body, { exact: true }).waitFor();
    assert.equal(await stack.evaluate(node => node.contains(document.activeElement)), false);
    assert.equal(await stack.getByRole('button', { name: B.actions[0].label }).count(), 0);
    await visible(true);
    await event(A.id, 'displayed');
    await visible(false);
    await visible(true);
    assert.deepEqual(await reports(), [['first-grant', A.id, 'displayed']]);
    await page.screenshot({ path: join(artifacts, 'account-notifications.png') });

    await button(front, '查看账号').click();
    await event(A.id, 'cta_clicked');
    await account.getByRole('heading', { name: 'DeepSeek 账号' }).waitFor();
    await pending('list');
    await show([B]);
    await front.getByText(A.title, { exact: true }).waitFor();
    assert.equal(await stack.getAttribute('data-interactive'), 'false');
    await page.setViewportSize({ width: 1120, height: 740 });
    await page.waitForFunction(() => document.querySelector('#account-notifications').style.clipPath.startsWith('path('));
    await page.screenshot({ path: join(artifacts, 'notice-under-settings.png') });
    await page.locator('#settings-shell').getByRole('button', { name: '关闭设置', exact: true }).click();
    await page.setViewportSize({ width: 1440, height: 1100 });
    await button(front, '查看账号').focus();
    await page.evaluate(() => { document.documentElement.lang = 'en-US'; });
    await pending('content', 'first-grant', A.id);
    await pending('content', 'first-grant', B.id);
    const english = make(A.id, 'Bonus credited', 'View account', 'English notice');
    await settle('content', english, 'first-grant', A.id);
    await settle('content', make(B.id, 'Balance reminder', 'View balance'), 'first-grant', B.id);
    await front.getByText(english.title, { exact: true }).waitFor();
    assert.equal(await button(front, 'View account').evaluate(node => node === document.activeElement), true);
    assert.equal((await reports()).filter(([, id, type]) => id === A.id && type === 'displayed').length, 1);
    await button(front, 'Close').click();
    await event(A.id, 'dismissed');
    await front.getByText('Balance reminder', { exact: true }).waitFor();
    await event(B.id, 'displayed');

    await button(front, 'View balance').click();
    await pending('list');
    await page.evaluate(() => {
      window.__verification.accountState.notifications.scope = 'second-grant';
      window.__verification.emit({ kind: 'deepseek_account_changed', reason: 'changed' });
    });
    await pending('list', 'second-grant');
    await stack.waitFor({ state: 'hidden' });
    await show([make('stale', '过时账号通知', '不应展示')]);
    assert.equal(await stack.isVisible(), false);
    await show([make('fresh', '新账号通知', '查看')], 'second-grant');
    await front.getByText('新账号通知', { exact: true }).waitFor();
    await event('fresh', 'displayed');
    assert.equal((await reports()).some(([, id]) => id === 'stale'), false);

    await page.goto(`${url}?scenario=intro`);
    await page.getByRole('heading', { name: '欢迎使用 KWork', exact: true }).waitFor();
    await pending('list');
    await show([A]);
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(await stack.isVisible(), false);
    assert.deepEqual(await reports(), []);
  } else if (mode === 'desktop-onboarding') {
    page.setDefaultTimeout(16000);
    const onboarding = page.locator('#desktop-onboarding');
    const platform = page.locator('#account-platform-view');
    const button = (scope, name) => scope.getByRole('button', { name, exact: true });
    const heading = text => onboarding.getByRole('heading', { name: text, exact: true });
    const currentProgress = { version: 1, step: 'welcome', purpose: null, process: null,
      completion: null, usage: 'compact', developerTools: false };
    let progress = { ...currentProgress };
    let revision = 0;
    const pending = async action => {
      await page.waitForFunction(() => typeof window.__verification.settleOnboarding === 'function');
      const request = await page.evaluate(() => window.__verification.calls.findLast(call =>
        call.name === 'desktop_onboarding').args);
      assert.deepEqual(request, { action, revision });
    };
    const settle = async (fields, error) => {
      if (!error) {
        progress = { ...progress, ...fields };
        revision += 1;
      }
      await page.evaluate(({ progress, revision, error }) => window.__verification.settleOnboarding(
        { progress, revision }, error), { progress, revision, error });
    };
    const balance = async (normal, bonus = []) => {
      await page.waitForFunction(() => window.__verification.accountRequests?.details?.length > 0);
      await page.evaluate(({ normal, bonus }) => {
        const account = window.__verification;
        const ready = { status: 'ready', value: normal.map(balance => ({ currency: 'CNY', balance })),
          bonusWallets: bonus.map(balance => ({ currency: 'USD', balance })) };
        account.accountRequests.details.shift().resolve({ ...account.accountState, details: { balance: ready } });
      }, { normal, bonus });
    };

    await heading('欢迎使用 KWork').waitFor();
    await page.waitForFunction(() => [...document.querySelectorAll('#desktop-onboarding img')]
      .some(image => image.getClientRects().length && image.complete && image.naturalWidth > 0));
    await page.screenshot({ path: join(artifacts, 'onboarding-welcome.png') });
    assert.equal(await page.locator('canvas').first().evaluate(node => node.inert), true);
    await balance(['0.0'], ['0e-5']);
    await button(onboarding, '开始设置').click();
    await pending({ action: 'next' });
    await settle({ step: 'credit' });
    await heading('准备可用额度').waitFor();
    await button(onboarding, '下一步').click();
    const noCredit = onboarding.getByRole('alertdialog', { name: '暂时跳过充值？' });
    await noCredit.waitFor();
    await page.keyboard.press('Escape');
    await noCredit.waitFor({ state: 'hidden' });
    assert.equal(await button(onboarding, '下一步').evaluate(node => node === document.activeElement), true);
    await button(onboarding, '去充值').click();
    await platform.getByRole('heading', { name: '充值', exact: true }).waitFor();
    assert.equal(await onboarding.evaluate(node => node.inert), true);
    await page.waitForFunction(() => window.__verification.platformRequests?.['open:1']);
    await page.evaluate(() => {
      window.__verification.platformRequests['open:1'].resolve(null);
      window.__verification.emit({ kind: 'account_platform_status', shell: 11, id: 1, status: 'loaded' });
    });
    await button(platform, '返回 KWork').click();
    await platform.waitFor({ state: 'hidden' });
    await balance([], ['0.01']);
    await button(onboarding, '下一步').click();
    await pending({ action: 'next' });
    await settle({ step: 'purpose' });
    const office = onboarding.getByRole('checkbox', { name: '办公与创作' });
    const development = onboarding.getByRole('checkbox', { name: '代码与开发' });
    await office.click();
    await pending({ action: 'select-purpose', purpose: 'office' });
    await settle({ purpose: 'office' });
    await development.click();
    await pending({ action: 'select-purpose', purpose: 'both' });
    await settle({}, '模拟保存失败，请重试。');
    await onboarding.getByRole('alert').getByText('模拟保存失败，请重试。').waitFor();
    assert.equal(await office.isChecked(), true);
    assert.equal(await development.isChecked(), false);
    await button(onboarding, '重试').click();
    await pending({ action: 'select-purpose', purpose: 'both' });
    await settle({ purpose: 'both' });
    await button(onboarding, '继续').click();
    await pending({ action: 'next' });
    await settle({ step: 'process' });
    const detailed = onboarding.getByRole('radio', { name: '完整过程' });
    await detailed.click();
    await pending({ action: 'select-process', process: 'detailed' });
    await settle({ process: 'detailed' });
    await page.reload({waitUntil:'domcontentloaded',timeout:60000});
    await heading('你希望看到多少工作过程？').waitFor();
    assert.equal(await detailed.isChecked(), true);
    await page.setViewportSize({ width: 760, height: 640 });
    await page.screenshot({ path: join(artifacts, 'onboarding-process-compact.png') });
    await button(onboarding, '进入应用').click();
    await pending({ action: 'finish', completion: 'completed' });
    await settle({ step: 'done', completion: 'completed', usage: 'detailed', developerTools: true });
    await onboarding.waitFor({ state: 'hidden' });
    await page.reload({waitUntil:'domcontentloaded',timeout:60000});
    await page.getByRole('button', { name: '更多设置', exact: true }).or(
      page.getByRole('button', { name: /账号菜单/ })).first().waitFor();
    assert.equal(await onboarding.isVisible(), false);
    assert.equal(await page.locator('canvas').first().evaluate(node => node.inert), false);

    for (const scenario of ['office', 'skip', 'api-key']) {
      await page.goto(`${url}?scenario=${scenario}`);
      revision = 0;
      progress = { ...currentProgress, ...(scenario === 'office' ? { step: 'purpose' }
        : scenario === 'skip' ? { step: 'credit' } : {}) };
      if (scenario === 'office') {
        await office.click();
        await pending({ action: 'select-purpose', purpose: 'office' });
        await settle({ purpose: 'office' });
        await button(onboarding, '继续').click();
        await pending({ action: 'finish', completion: 'completed' });
        await settle({ step: 'done', completion: 'completed', process: 'compact' });
      } else if (scenario === 'skip') {
        await button(onboarding, '跳过').click();
        await button(onboarding.getByRole('alertdialog'), '我知道了').click();
        await pending({ action: 'finish', completion: 'skipped' });
        await settle({ step: 'done', completion: 'skipped', process: 'standard' });
      } else {
        await pending({ action: 'finish', completion: 'api-key' });
        await settle({ step: 'done', completion: 'api-key', process: 'standard', usage: 'detailed', developerTools: true });
      }
      await onboarding.waitFor({ state: 'hidden' });
    }
  } else if (mode === 'deepseek-account') {
    page.setDefaultTimeout(12000);
    await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
    const welcome = page.locator('#welcome-setup');
    const account = page.locator('#deepseek-account-form');
    const launcher = page.locator('#deepseek-account-launcher');
    const menu = launcher.getByRole('menu', { name: '账号菜单' });
    const trigger = launcher.locator(':scope > button');
    const key = welcome.locator('input[type=password]');
    const button = (region, name) => region.getByRole('button', { name, exact: true });
    const waitsFor = text => account.getByRole('status').getByText(text, { exact: true }).waitFor();
    const pending = async (action, count = 1) => {
      await page.waitForFunction(({ action, count }) => window.__verification.accountRequests?.[action]?.length === count, { action, count });
      return page.evaluate(verb => window.__verification.calls.findLast(call =>
        call.name === 'deepseek_account' && call.args.request.action === verb).args.request, action);
    };
    const settle = (action, reply, projected = reply) => page.evaluate(({ action, reply, projected }) => {
      const request = window.__verification.accountRequests[action].shift();
      if (!reply) request.reject('fixture account error');
      else {
        if (projected) window.__verification.accountState = projected;
        request.resolve(reply);
      }
    }, { action, reply, projected });
    const observed = reply => page.evaluate(value => { window.__verification.accountState = value; }, reply);
    const links = { usageUrl: 'https://fixture.invalid/usage', topUpUrl: 'https://fixture.invalid/top_up' };
    const snapshot = (attempt = null, signedIn = false) => ({ available: true, platformEmbedded: true, modelReady: signedIn,
      state: { status: signedIn ? 'credential-stored' : 'signed-out', links, attempt } });
    const initial = id => ({ id, phase: 'initializing' });
    const waiting = (id, url) => ({ id, phase: 'waiting-browser', authorizeUrl: url });
    const opened = href => page.waitForFunction(url => window.__verification.calls.some(call => call.name === 'open_browser_url' && call.args.url === url), href);
    const themed = async raw => {
      const url = new URL(raw);
      url.searchParams.set('theme', await account.getAttribute('data-appearance'));
      return url.href;
    };
    const avatarUrl = 'https://fixture.invalid/deepseek-owner.png';
    let avatarReads = 0;
    await page.route(avatarUrl, route => {
      avatarReads += 1;
      return route.fulfill({ contentType: 'image/png', body: avatarReads === 1
        ? Buffer.from('corrupt fixture PNG') : Buffer.from(png, 'base64') });
    });
    const details = { profile: { status: 'ready', value: { id: 'fixture-user', name: 'Fixture Owner',
      contact: 'fi***@example.invalid', avatarUrl } },
    balance: { status: 'ready', value: [{ currency: 'CNY', balance: '72.9469891200000000' },
      { currency: 'USD', balance: '0E-16' }], bonusWallets: [{ currency: 'USD', balance: '2.00' }] } };

    await key.fill('draft-to-keep');
    await button(welcome, '登录 DeepSeek 账号').click();
    await pending('start');
    const first = waiting('first-attempt', 'https://fixture.invalid/dsh/authorize?first');
    await settle('start', snapshot(initial(first.id)), snapshot(first));
    await waitsFor('请在浏览器中完成登录。');
    const firstLink = await themed(first.authorizeUrl);
    await opened(firstLink);
    await button(account, '复制授权链接').click();
    await waitsFor('已复制授权链接。');
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), firstLink);
    assert.equal(await welcome.evaluate(node => node.inert), true);
    await button(account, '返回').click();
    assert.equal((await pending('cancel')).attempt, first.id);
    await settle('cancel', snapshot({ id: first.id, phase: 'cancelled' }));
    await account.waitFor({ state: 'hidden' });
    assert.equal(await key.inputValue(), 'draft-to-keep');
    assert.equal(await welcome.evaluate(node => node.inert), false);

    await button(welcome, '登录 DeepSeek 账号').click();
    await pending('start');
    const second = waiting('second-attempt', 'https://fixture.invalid/dsh/authorize?second');
    await settle('start', snapshot(initial(second.id)), snapshot(second));
    await opened(await themed(second.authorizeUrl));
    await waitsFor('请在浏览器中完成登录。');
    const signed = snapshot({ id: second.id, phase: 'succeeded' }, true);
    await observed(signed);
    await waitsFor('已登录 DeepSeek。');
    await button(account, '继续').click();
    await welcome.waitFor({ state: 'hidden' });
    await account.waitFor({ state: 'hidden' });

    await pending('details');
    await settle('details', { ...signed, details }, signed);
    await launcher.locator('.account-avatar[data-avatar-state="failed"]').waitFor();
    await launcher.getByRole('button', { name: '账号菜单，Fixture Owner', exact: true }).waitFor();
    await trigger.focus();
    await trigger.press('ArrowDown');
    const settings = menu.getByRole('menuitem', { name: '设置', exact: true });
    await settings.waitFor();
    await page.screenshot({ path: join(artifacts, 'account-launcher.png') });
    assert.equal(await settings.evaluate(node => node === document.activeElement), true);
    await settings.press('ArrowDown');
    assert.equal(await menu.getByRole('menuitem', { name: '联系我们' }).evaluate(node => node === document.activeElement), true);
    await page.keyboard.press('Escape');
    await menu.waitFor({ state: 'hidden' });
    assert.equal(await trigger.evaluate(node => node === document.activeElement), true);
    await trigger.click();
    await menu.getByRole('menuitem', { name: '联系我们' }).click();
    await page.waitForFunction(() => window.__verification.calls.some(call =>
      call.name === 'open_browser_url' && call.args.url.startsWith('https://trtgsjkv6r.feishu.cn/')));
    const contact = new URL(await page.evaluate(() => window.__verification.calls.findLast(call =>
      call.name === 'open_browser_url').args.url));
    assert.equal(contact.hostname, 'trtgsjkv6r.feishu.cn');
    assert.equal(contact.searchParams.get('prefill_uid'), 'fixture-user');
    assert.equal(contact.searchParams.get('hide_uid'), '1');
    await trigger.click();
    await settings.click();
    const nav = page.locator('#settings-shell');
    await button(nav, '账号与余额').dispatchEvent('click');
    await pending('details');
    await settle('details', { ...signed, details }, signed);
    await account.getByText('Fixture Owner', { exact: true }).waitFor();
    await account.locator('.account-avatar img').waitFor();
    await account.getByText('¥72.94', { exact: true }).waitFor();
    await account.getByText('$0.00', { exact: true }).waitFor();
    await account.getByText('$2.00', { exact: true }).waitFor();
    await account.getByRole('link', { name: '更多账号信息' }).click();
    await opened('https://fixture.invalid/');
    await page.setViewportSize({ width: 1008, height: 822 });
    await page.waitForFunction(() => document.querySelector('#deepseek-account-form')?.getBoundingClientRect().top === document.querySelector('#settings-shell')?.getBoundingClientRect().top + 54);
    await page.screenshot({ path: join(artifacts, 'account-settings.png') });
    await page.setViewportSize({ width: 1440, height: 1100 });
    const platform = page.locator('#account-platform-view');
    const back = button(platform, '返回 KWork');
    const pendingPlatform = async (action, after = 0) => {
      await page.waitForFunction(({ action, after }) => Object.keys(window.__verification.platformRequests ?? {}).some(key =>
        key.startsWith(`${action}:`) && Number(key.split(':')[1]) > after), { action, after });
      return page.evaluate(({ action, after }) => window.__verification.calls.findLast(call =>
        call.name === 'account_platform_view' && call.args.request.action === action && call.args.request.id > after).args.request, { action, after });
    };
    const settlePlatform = (action, id, fail = false) => page.evaluate(({ action, id, fail }) => {
      const key = `${action}:${id}`;
      const { resolve, reject } = window.__verification.platformRequests[key];
      delete window.__verification.platformRequests[key];
      if (fail) reject('fixture could not load');
      else resolve(null);
    }, { action, id, fail });
    const platformStatus = (id, status, shell = 11) => page.evaluate(({ id, status, shell }) =>
      window.__verification.emit({ kind: 'account_platform_status', shell, id, status }), { id, status, shell });
    const usageLink = account.getByRole('link', { name: '查询用量', exact: true });
    await usageLink.click();
    const usage = await pendingPlatform('open');
    assert.equal(usage.page, 'usage');
    assert.deepEqual(Object.keys(usage).sort(), ['action', 'bounds', 'id', 'page']);
    assert.equal(await account.evaluate(node => node.inert), true);
    assert.equal(await back.evaluate(node => node === document.activeElement), true);
    await settlePlatform('open', usage.id, true);
    await platform.getByText('未能加载 DeepSeek 平台页面。', { exact: true }).waitFor();
    await page.screenshot({ path: join(artifacts, 'account-platform-retry.png') });
    await button(platform, '重试').click();
    const retry = await pendingPlatform('open', usage.id);
    assert.equal(retry.page, 'usage');
    await platformStatus(usage.id, 'loaded');
    await platform.getByText('正在加载…', { exact: true }).waitFor();
    await settlePlatform('open', retry.id);
    await platformStatus(retry.id, 'loaded');
    await platform.getByRole('status').waitFor({ state: 'hidden' });
    await page.setViewportSize({ width: 1160, height: 860 });
    await page.waitForFunction(id => window.__verification.calls.some(call =>
      call.name === 'account_platform_view' && call.args.request.action === 'bounds'
      && call.args.request.id === id && call.args.request.bounds.width === 1160), retry.id);
    await page.evaluate(() => { window.__verification.holdPlatformClose = true; });
    await back.click();
    await pendingPlatform('close', usage.id);
    assert.equal(await account.evaluate(node => node.inert), true);
    await settlePlatform('close', retry.id);
    await platform.waitFor({ state: 'hidden' });
    assert.equal(await usageLink.evaluate(node => node === document.activeElement), true);
    const readsBeforeTopUp = await page.evaluate(() => window.__verification.calls.filter(call =>
      call.name === 'deepseek_account' && call.args.request.action === 'details').length);
    assert.equal(readsBeforeTopUp, 2);

    await page.evaluate(() => window.__verification.emit({ kind: 'deepseek_account_changed', reason: 'changed' }));
    await pending('details');
    await account.getByRole('link', { name: '充值', exact: true }).click();
    const topUp = await pendingPlatform('open', retry.id);
    assert.equal(topUp.page, 'top-up');
    await back.click();
    await pendingPlatform('close', retry.id);
    await settlePlatform('close', topUp.id);
    await platform.waitFor({ state: 'hidden' });
    await settlePlatform('open', topUp.id);
    await platformStatus(topUp.id, 'loaded');
    await settle('details', { ...signed, details }, signed);
    await pending('details');
    const refreshed = { ...details, balance: { ...details.balance, value: [{ currency: 'CNY', balance: '12.50' }] } };
    await settle('details', { ...signed, details: refreshed }, signed);
    await account.getByText('¥12.50', { exact: true }).waitFor();
    assert.equal(await platform.isVisible(), false);
    await page.evaluate(() => { window.__verification.holdPlatformClose = false; });
    await page.setViewportSize({ width: 1440, height: 1100 });
    await usageLink.click();
    const beforeChange = await pendingPlatform('open', topUp.id);
    await settlePlatform('open', beforeChange.id);
    await platformStatus(beforeChange.id, 'loaded');
    await platform.getByRole('status').waitFor({ state: 'hidden' });
    await page.evaluate(() => window.__verification.emit({ kind: 'deepseek_account_changed', reason: 'changed' }));
    await pending('details');
    assert.equal(await platform.isVisible(), true, 'an account refresh keeps the current grant view');
    await platformStatus(beforeChange.id, 'revoked', 10);
    await platformStatus(topUp.id, 'revoked');
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(await platform.isVisible(), true, 'revocation must match both the shell and visit');
    await platformStatus(beforeChange.id, 'revoked');
    await platform.waitFor({ state: 'hidden' });
    assert.equal(await page.evaluate(id => window.__verification.calls.some(call =>
      call.name === 'account_platform_view' && call.args.request.action === 'close' && call.args.request.id === id), beforeChange.id), false);
    await settle('details', { ...signed, details: refreshed }, signed);

    await page.locator('#settings-shell').getByRole('button', { name: '关闭设置', exact: true }).click();
    const openSignout = async () => {
      await trigger.click();
      await menu.getByRole('menuitem', { name: '退出登录' }).click();
    };
    await openSignout();
    await pending('check-signout');
    await settle('check-signout', { ...signed, runningTasks: true }, signed);
    const confirm = account.getByRole('alertdialog', { name: '退出登录', exact: true });
    await confirm.getByText(/当前有进程正在执行中/).waitFor();
    assert.equal(await button(confirm, '关闭').evaluate(node => node === document.activeElement), true);
    await page.keyboard.press('Shift+Tab');
    assert.equal(await button(confirm, '退出登录').evaluate(node => node === document.activeElement), true);
    await page.keyboard.press('Tab');
    assert.equal(await button(confirm, '关闭').evaluate(node => node === document.activeElement), true);
    const { width, height } = page.viewportSize();
    assert.deepEqual(await account.locator('.account-confirm-mask').boundingBox(), { x: 0, y: 0, width, height });
    assert.equal((await confirm.boundingBox()).width, 380);
    await page.screenshot({ path: join(artifacts, 'account-signout.png'), animations: 'disabled' });
    await page.keyboard.press('Escape');
    await confirm.waitFor({ state: 'hidden' });
    await openSignout();
    await pending('check-signout');
    await settle('check-signout', { ...signed, runningTasks: false }, signed);
    await confirm.getByText('退出登录不会丢失任何数据，你仍可以登录此账号。', { exact: true }).waitFor();
    await page.setViewportSize({ width: 1008, height: 822 });
    await page.screenshot({ path: join(artifacts, 'account-signout-idle.png'), animations: 'disabled' });
    await page.setViewportSize({ width, height });
    await button(confirm, '退出登录').click();
    assert.equal((await pending('signout')).approveRunning, false);
    await settle('signout', { ...signed, needsConfirmation: true, runningTasks: true }, signed);
    await confirm.getByText(/当前有进程正在执行中/).waitFor();
    await button(confirm, '退出登录').click();
    assert.equal((await pending('signout')).approveRunning, true);
    await confirm.dispatchEvent('keydown', { key: 'Escape', bubbles: true });
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(await confirm.isVisible(), true);
    await settle('signout', snapshot());
    await confirm.waitFor({ state: 'hidden' });
    await account.waitFor({ state: 'hidden' });
    assert.equal(await account.getByText('Fixture Owner', { exact: true }).isVisible(), false);
    await trigger.click();
    await menu.getByRole('menuitem', { name: '登录 DeepSeek 账号' }).click();
    await pending('start');
    const third = waiting('sidebar-attempt', 'https://fixture.invalid/dsh/authorize?sidebar');
    await settle('start', snapshot(third));
    await waitsFor('请在浏览器中完成登录。');
    await page.keyboard.press('Escape');
    assert.equal((await pending('cancel')).attempt, third.id);
    await settle('cancel', snapshot({ id: third.id, phase: 'cancelled' }));
    await account.waitFor({ state: 'hidden' });
    assert.equal(await trigger.evaluate(node => node === document.activeElement), true);
    await trigger.click();
    await menu.getByRole('menuitem', { name: '登录 DeepSeek 账号' }).click();
    await pending('start');
    await settle('start', snapshot({ id: 'expired-attempt', phase: 'expired' }));
    await waitsFor('登录已过期，请重新开始。');
    await button(account, '添加 API Key').click();
    await account.waitFor({ state: 'hidden' });
    await nav.getByRole('button', { name: /^模型(?:服务)?$/ }).waitFor();

    await page.goto(`${url}?scenario=restored`);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'start_service'));
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'new_session' || call.name === 'resume_session'));
    await welcome.waitFor({ state: 'hidden' });
    await pending('details');
    await page.evaluate(() => window.__verification.emit({ kind: 'deepseek_account_changed', reason: 'changed' }));
    await pending('details', 2);
    await settle('details', { ...signed, details: { ...details,
      profile: { status: 'ready', value: { name: 'Old Account' } } } }, null);
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(await launcher.getByText('Old Account').isVisible(), false);
    await settle('details', { ...signed, details }, null);
    await launcher.getByRole('button', { name: '账号菜单，Fixture Owner', exact: true }).waitFor();
    assert.equal(await launcher.getByText('Old Account').isVisible(), false);
    await launcher.locator('.account-avatar img').waitFor();
    await trigger.click();
    await menu.getByRole('menuitem', { name: '退出登录' }).click();
    await pending('check-signout');
    await settle('check-signout', null);
    await confirm.getByText(/暂时无法确认任务状态/).waitFor();
    await button(confirm, '取消').click();
    await account.waitFor({ state: 'hidden' });
    assert.equal(await trigger.evaluate(node => node === document.activeElement), true);
    await trigger.click();
    await menu.getByRole('menuitem', { name: '退出登录' }).click();
    await pending('check-signout');
    await settle('check-signout', { ...signed, runningTasks: false }, signed);
    await button(confirm, '退出登录').click();
    await pending('signout');
    await settle('signout', snapshot());
    await launcher.getByRole('button', { name: '更多设置', exact: true }).waitFor();
    assert.equal(await launcher.locator('.account-avatar img').count(), 0);
    await page.goto(`${url}?scenario=legacy`);
    await pending('details');
    await settle('details', { ...signed, platformEmbedded: false, details }, { ...signed, platformEmbedded: false });
    await trigger.click();
    await settings.click();
    await button(nav, '账号与余额').dispatchEvent('click');
    await usageLink.click();
    await opened(links.usageUrl);
    assert.equal(await platform.isVisible(), false);
  } else if (mode === 'provider-authorization') {
    page.setDefaultTimeout(10000);
    await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
    const form = page.getByRole('region', { name: '模型服务', exact: true });
    const dialog = form.getByRole('dialog', { name: '登录服务商 Codex fixture', exact: true });
    const logout = form.getByRole('dialog', { name: '退出登录 Codex fixture', exact: true });
    const text = dialog.locator('[data-part="login-answer"]');
    const button = (surface, name) => surface.getByRole('button', { name, exact: true });
    const stat = (surface, message) => surface.getByRole('status').getByText(message, { exact: true });
    const pending = async (action, verb) => {
      await page.waitForFunction(() => typeof window.__verification.settleProviderSettings === 'function');
      const request = await page.evaluate(() => window.__verification.calls.filter(call =>
        call.name === 'model_providers' && call.args.draft?.action !== 'status').at(-1).args);
      assert.equal(request.action, action);
      if (verb) assert.equal(request.draft.action, verb);
      return request.draft;
    };
    const settle = (answer, projected = answer) => page.evaluate(({ answer, projected }) => {
      window.__verification.providerLoginState = projected;
      window.__verification.settleProviderSettings(answer, null);
    }, { answer, projected });
    const route = { id: 'openai-codex', name: 'Codex fixture', baseUrl: '', keyConfigured: false,
      available: false, editable: true, removable: false, canClearKey: false, models: [],
      catalog: { defaultBaseUrl: 'https://fixture.invalid/v1', endpointEnv: [], authEnv: [], authDetected: false,
        oauth: true, oauthConfigured: false, signedIn: false, apiKeyAllowed: false, inheritedModels: [], modelsOverridden: false,
        loginMethods: [{ id: 'oauth', label: 'Continue in browser' }, { id: 'api-key', label: 'Enter API key' }],
      },
    };
    await page.getByRole('button', { name: '配置模型服务', exact: true }).click();
    await pending('list');
    await settle({ providers: [], catalogProviders: [route], revision: 12 });
    await button(form, '第三方模型服务商').click();
    await button(form, route.name).click();
    const endpoint = form.getByLabel('API 地址', { exact: true });
    await endpoint.fill('https://edited.fixture.invalid/v1');
    await button(form, '登录服务商').click();
    const method = dialog.getByRole('combobox', { name: '登录方式' });
    await method.waitFor();
    assert.equal(await method.inputValue(), 'oauth');
    assert.equal(await form.locator('.provider-main').evaluate(main => main.inert), true);
    await button(dialog, '开始登录').click();
    assert.deepEqual(await pending('authorize', 'start'), { id: route.id, action: 'start', method: 'oauth' });

    const running = { id: 'attempt-fixture', revision: 1, status: 'running', url: 'https://fixture.invalid/verify',
      code: 'ABCD-EFGH', instruction: 'Enter the device code', prompt: { id: 'select', kind: 'select', message: 'Which account?',
        options: [{ id: 'work', label: 'Work' }, { id: 'personal', label: 'Personal', description: 'Fixture account' }],
      },
    };
    await settle(running);
    await dialog.getByText('设备码 ABCD-EFGH', { exact: true }).waitFor();
    await button(dialog, '在浏览器中打开').click();
    await page.waitForFunction(() => window.__verification.calls.some(({ name, args }) => name === 'open_browser_url' && args.url === 'https://fixture.invalid/verify'));
    await button(dialog, '复制设备码').click();
    await button(dialog, '已复制设备码').waitFor();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), 'ABCD-EFGH');
    await dialog.getByRole('combobox', { name: 'Which account?' }).selectOption('personal');
    await button(dialog, '提交').click();
    assert.deepEqual(await pending('authorize', 'answer'), { id: route.id, action: 'answer', attempt: running.id, prompt: 'select', value: 'personal' });
    const masked = { ...running, revision: 4, prompt: { id: 'masked', kind: 'secret', message: 'Enter fixture token', placeholder: 'Token' } };
    await settle(masked, running);
    const statusReads = () => page.evaluate(() => window.__verification.calls.filter(call => call.args.draft?.action === 'status').length);
    await page.waitForFunction(prior => window.__verification.calls.filter(call => call.args.draft?.action === 'status').length > prior, await statusReads());
    assert.equal(await text.getAttribute('type'), 'password');
    await text.fill('fixture-secret-only');
    await page.screenshot({ path: join(artifacts, 'provider-device-login.png') });
    await button(dialog, '提交').click();
    const reply = await pending('authorize', 'answer');
    assert.equal(reply.prompt, 'masked');
    assert.equal(reply.value, 'fixture-secret-only');
    assert.equal(await text.inputValue(), '');
    const manual = { ...running, revision: 6, prompt: { id: 'manual', kind: 'text', message: 'Optional callback URL' } };
    await settle(manual);
    await dialog.getByRole('textbox', { name: 'Optional callback URL', exact: true }).waitFor();
    assert.equal(await text.getAttribute('type'), 'text');
    await page.evaluate(state => { window.__verification.providerLoginState = state; }, { ...manual, revision: 7, prompt: null });
    await text.waitFor({ state: 'hidden' });
    await page.evaluate(state => { window.__verification.providerLoginState = state; }, { id: running.id, revision: 8, status: 'authorized', credentialKind: 'grant' });
    await stat(dialog, '登录成功。').waitFor();
    assert.equal(await button(dialog, '在浏览器中打开').isVisible(), false);
    await button(dialog, '完成').click();
    assert.equal(await endpoint.inputValue(), 'https://edited.fixture.invalid/v1');
    await button(form, '重新登录服务商').waitFor();

    await button(form, '退出服务商登录').click();
    await logout.waitFor();
    await page.keyboard.press('Escape');
    await logout.waitFor({ state: 'hidden' });
    await button(form, '退出服务商登录').click();
    await button(logout, '确认退出').click();
    assert.deepEqual(await pending('authorize', 'logout'), { id: route.id, action: 'logout' });
    await settle({ revision: 0, status: 'signed-out' });
    await stat(logout, '已退出登录。').waitFor();
    await button(logout, '完成').click();
    await button(form, '退出服务商登录').waitFor({ state: 'hidden' });
    assert.equal(await endpoint.inputValue(), 'https://edited.fixture.invalid/v1');

    await button(form, '登录服务商').click();
    await method.selectOption('api-key');
    await button(dialog, '开始登录').click();
    assert.equal((await pending('authorize', 'start')).method, 'api-key');
    const second = { ...masked, id: 'attempt-second', revision: 1 };
    await settle(second);
    await text.waitFor();
    await page.keyboard.press('Escape');
    assert.equal((await pending('authorize', 'cancel')).attempt, second.id);
    await settle({ id: second.id, revision: 2, status: 'cancelled' });
    await stat(dialog, '登录已取消。').waitFor();
    await button(dialog, '完成').click();
    await button(form, '登录服务商').waitFor();

    await button(form, '登录服务商').click();
    await button(dialog, '开始登录').click();
    await pending('authorize', 'start');
    const third = { id: 'attempt-fail', revision: 1, status: 'running' };
    await settle(third);
    await button(dialog, '取消登录').waitFor();
    await page.evaluate(state => { window.__verification.providerLoginState = state; }, { ...third, revision: 2, status: 'failed' });
    await stat(dialog, '登录失败，请重试。').waitFor();
    await button(dialog, '重试').evaluate(node => node.click());
    await button(dialog, '开始登录').evaluate(node => node.click());
    await pending('authorize', 'start');
    const retried = { id: 'attempt-retry', revision: 1, status: 'running' };
    await settle(retried);
    await button(dialog, '取消登录').waitFor();
    await page.evaluate(state => { window.__verification.providerLoginState = state; }, { ...retried, revision: 2, status: 'authorized', credentialKind: 'grant' });
    await stat(dialog, '登录成功。').waitFor();
    await button(dialog, '完成').click();
  } else if (mode === 'provider-protocols') {
    page.setDefaultTimeout(10000);
    const standard = ['openai-completions', 'openai-responses', 'anthropic-messages'];
    const gemini = 'google-generative-ai';
    const form = page.getByRole('region', { name: '模型服务', exact: true });
    const protocol = form.getByRole('combobox', { name: '接口协议', exact: true });
    const choices = () => protocol.locator('option').evaluateAll(options => options.map(option => option.value));
    const address = form.getByLabel('API 地址', { exact: true });
    const key = form.locator('[data-field="key"]');
    const keyError = form.getByRole('alert').getByText('请输入当前 Gemini API 地址的密钥。');
    const sync = form.getByRole('button', { name: '同步模型', exact: true });
    const save = form.getByRole('button', { name: '保存服务商', exact: true });
    const issue = form.getByRole('alert').getByText(/Gemini API 仅接受/);
    const pending = async action => {
      await page.waitForFunction(() => typeof window.__verification.settleProviderSettings === 'function');
      const request = await page.evaluate(() => window.__verification.calls.filter(call => call.name === 'model_providers').at(-1).args);
      assert.equal(request.action, action);
      return request.draft;
    };
    const settle = (answer, error = null) => page.evaluate(({ answer, error }) => window.__verification.settleProviderSettings(answer, error), { answer, error });
    const open = async (protocols, providers, revision, catalogProviders) => {
      await page.getByRole('button', { name: '配置模型服务', exact: true }).click();
      await pending('list');
      await settle({ providers, protocols, revision, ...(catalogProviders === undefined ? {} : { catalogProviders }) });
      await protocol.locator(`option[value="${protocols.at(-1)}"]`).waitFor({ state: 'attached' });
    };
    await open(standard, []);
    assert.deepEqual(await choices(), standard);
    assert.equal(await form.getByRole('button', { name: 'Gemini', exact: true }).count(), 0);
    await form.getByRole('button', { name: 'Anthropic', exact: true }).click();
    await page.waitForFunction(() => document.querySelector('[data-field="url"]')?.value === 'https://api.anthropic.com');
    assert.equal(await protocol.inputValue(), 'anthropic-messages');
    await address.fill('http://[::1]');
    await sync.and(form.locator(':enabled')).waitFor();
    await address.fill('https://fixture.invalid/v1/messages');
    await form.getByRole('alert').getByText(/有效的 HTTP 或 HTTPS 基础地址/).waitFor();
    assert.equal(await sync.isDisabled(), true);

    await page.reload();
    await open([...standard, gemini], [{ id: 'saved-gemini', name: 'Saved Gemini fixture', protocol: gemini,
      baseUrl: 'https://gemini.fixture.invalid/v1beta', available: true, keyConfigured: true, models: [{ id: 'gemini-fixture' }] }]);
    assert.deepEqual(await choices(), [...standard, gemini]);
    assert.equal(await key.inputValue(), '');
    assert.equal(await sync.isEnabled(), true);
    await address.fill('https://gemini.fixture.invalid:443/alternate/v1beta');
    await sync.click();
    const storedProbe = await pending('discover');
    assert.equal(storedProbe.id, 'saved-gemini');
    assert.equal(storedProbe.protocol, gemini);
    assert.equal(storedProbe.apiKey, undefined);
    await settle([]);
    await form.getByRole('status').getByText(/没有列出任何模型/).waitFor();
    await address.fill('http://gemini.fixture.invalid/v1beta');
    await keyError.waitFor();
    assert.equal(await sync.isDisabled(), true);
    assert.equal(await save.isDisabled(), true);
    await address.fill('https://gemini.fixture.invalid/v1beta');
    await keyError.waitFor({ state: 'hidden' });
    await form.getByRole('button', { name: '清除已保存密钥', exact: true }).click();
    await keyError.waitFor();
    assert.equal(await save.isDisabled(), true);
    await form.getByRole('button', { name: '保留已保存密钥', exact: true }).click();
    await keyError.waitFor({ state: 'hidden' });

    await form.getByRole('button', { name: 'Anthropic', exact: true }).click();
    await form.getByRole('button', { name: '手动添加', exact: true }).click();
    await form.getByLabel('模型 ID 1', { exact: true }).fill('gemini-custom');
    await form.getByRole('button', { name: '模型高级参数 gemini-custom', exact: true }).click();
    const reasoning = form.getByRole('group', { name: '推理等级 1', exact: true });
    const toggle = reasoning.getByRole('switch');
    await toggle.check();
    await reasoning.getByRole('textbox', { name: 'high API 参数值' }).fill('provider-specific');
    await protocol.selectOption(gemini);
    await keyError.waitFor();
    await issue.waitFor();
    await address.fill('https://gemini.fixture.invalid/v1beta');
    await key.fill('sk-fixture-gemini');
    await keyError.waitFor({ state: 'hidden' });
    assert.equal(await save.isDisabled(), true);
    await toggle.uncheck();
    await issue.waitFor({ state: 'hidden' });
    await toggle.check();
    assert.equal(await reasoning.getByRole('textbox', { name: 'off API 参数值' }).count(), 0);
    const geminiValues = await reasoning.getByRole('combobox').first().locator('option').evaluateAll(options => options.map(option => option.value));
    assert.ok(geminiValues.every(value => ['minimal', 'low', 'medium', 'high'].includes(value)));
    assert.equal(await reasoning.getByRole('textbox', { name: 'high API 参数值' }).isDisabled(), true);
    await reasoning.getByRole('button', { name: '添加档位' }).click();
    await reasoning.getByRole('textbox', { name: 'minimal API 参数值' }).waitFor();
    await sync.click();
    const probe = await pending('discover');
    assert.equal(probe.protocol, gemini);
    assert.equal(probe.apiKey, 'sk-fixture-gemini');
    await settle([{ id: 'gemini-second', name: 'Gemini second', contextWindow: 1000000 }]);
    const picker = form.getByRole('dialog', { name: '选择模型' });
    await picker.getByRole('checkbox', { name: 'gemini-second' }).check();
    await picker.getByRole('button', { name: '更新列表' }).click();
    await picker.waitFor({ state: 'hidden' });
    await page.screenshot({ path: join(artifacts, 'gemini-protocol.png') });
    await save.click();
    const draft = await pending('save');
    assert.equal(draft.protocol, gemini);
    assert.equal(draft.apiKey, 'sk-fixture-gemini');
    assert.deepEqual(draft.models.map(model => model.id), ['gemini-custom', 'gemini-second']);
    assert.deepEqual(draft.models[0].reasoningEfforts, [['low', 'low'], ['medium', 'medium'], ['high', 'high'], ['minimal', 'minimal']]);
    assert.equal(draft.models[1].contextWindow, 1000000);
    const { apiKey: _typedKey, ...profile } = draft;
    await settle({ id: 'new-gemini', providers: [{ ...profile, id: 'new-gemini', keyConfigured: true, available: true }] });
    await form.getByRole('status').getByText('模型服务已保存').waitFor();
    assert.equal(await key.inputValue(), '');
    assert.equal(await protocol.inputValue(), gemini);

    await page.reload();
    const shared = { id: 'official-gateway', name: 'Shared route', protocol: 'anthropic-messages',
      baseUrl: 'https://shared.fixture.invalid', keyConfigured: true, available: true,
      removable: true, canClearKey: true, models: [{ id: 'fixture-claude' }] };
    const deployed = { ...shared, id: 'deployed-gateway', name: '部署服务', editable: false, removable: false, canClearKey: false };
    await open(standard, [deployed, shared], 41);
    const clear = form.getByRole('button', { name: '清除已保存密钥', exact: true });
    const remove = form.getByRole('button', { name: '删除服务商', exact: true });
    const name = form.getByLabel('服务商名称', { exact: true });
    await form.getByRole('button', { name: deployed.name, exact: true, pressed: true }).waitFor();
    assert.equal(await clear.isVisible(), false);
    assert.equal(await remove.isVisible(), false);
    assert.equal(await name.isDisabled(), true);
    assert.equal(await save.isDisabled(), true);

    await form.getByRole('searchbox', { name: '搜索服务商' }).fill(shared.id);
    await form.getByRole('button', { name: deployed.name, exact: true }).waitFor({ state: 'hidden' });
    await form.getByRole('button', { name: shared.name, exact: true }).click();
    await clear.waitFor();
    await remove.waitFor();
    await name.fill('本地草稿名称');
    await save.click();
    const stale = await pending('save');
    assert.equal(stale.id, shared.id);
    assert.equal(stale.expectedRevision, 41);
    await settle(null, '模型服务配置已更新。请刷新列表并重新选择服务商。');
    await form.getByRole('status').getByText(/请刷新列表/).waitFor();
    assert.equal(await name.inputValue(), '本地草稿名称');
    await form.getByRole('button', { name: '刷新列表', exact: true }).click();
    await pending('list');
    const edited = { ...shared, name: '官方客户端修改' };
    await settle({ providers: [deployed, edited], protocols: standard, revision: 42 });
    await form.getByRole('status').getByText(/请重新选择服务商后再编辑/).waitFor();
    assert.equal(await name.inputValue(), '本地草稿名称');
    assert.equal(await save.isDisabled(), true);
    await form.getByRole('button', { name: edited.name, exact: true }).click();
    await page.waitForFunction(() => document.querySelector('[data-field="name"]')?.value === '官方客户端修改');
    await name.fill('已合并的名称');
    await save.click();
    const rebased = await pending('save');
    assert.equal(rebased.expectedRevision, 42);
    assert.equal(rebased.name, '已合并的名称');
    await settle({ id: shared.id, providers: [deployed, { ...edited, name: rebased.name }], revision: 43 });
    await form.getByRole('status').getByText('模型服务已保存').waitFor();

    await page.reload();
    const catalog = { id: 'openai', name: 'openai', baseUrl: '', keyConfigured: false,
      available: false, editable: true, removable: false, canClearKey: false, models: [],
      catalog: { defaultBaseUrl: 'https://api.openai.com/v1', endpointEnv: [], authEnv: ['OPENAI_API_KEY'],
        authDetected: true, oauth: false, oauthConfigured: false, apiKeyAllowed: true, inheritedModels: [], modelsOverridden: false },
    };
    const codex = { ...catalog, id: 'openai-codex', name: 'openai-codex', catalog: { ...catalog.catalog,
      authEnv: [], oauth: true, oauthConfigured: true, apiKeyAllowed: false },
    };
    await open(standard, [], 50, [catalog, codex]);
    const search = form.getByRole('searchbox', { name: '搜索服务商' });
    const directory = form.getByRole('group', { name: '可添加的目录服务商' });
    const expandDirectory = form.getByRole('button', { name: '第三方模型服务商', exact: true });
    await expandDirectory.waitFor();
    assert.equal(await expandDirectory.getAttribute('aria-expanded'), 'false');
    await expandDirectory.click();
    await directory.waitFor();
    await expandDirectory.click();
    await directory.waitFor({ state: 'hidden' });
    await search.fill('codex');
    await directory.getByRole('button', { name: 'openai-codex', exact: true }).click();
    await form.getByText('已检测到 Host 的服务商登录凭据。').waitFor();
    assert.equal(await key.isVisible(), false);
    assert.equal(await protocol.isVisible(), false);
    assert.equal(await save.isEnabled(), true);

    await search.fill('openai');
    await directory.getByRole('button', { name: 'openai', exact: true }).click();
    await form.getByText('已检测到环境认证 OPENAI_API_KEY。').waitFor();
    assert.equal(await address.getAttribute('placeholder'), 'https://api.openai.com/v1');
    assert.equal(await address.inputValue(), '');
    assert.equal(await name.isVisible(), false);
    await form.getByText('使用完整的内置模型目录。').waitFor();
    await address.fill('https://override.fixture.invalid/v1');
    await key.fill('sk-directory-fixture');
    await form.getByRole('button', { name: '选择目录模型' }).click();
    assert.deepEqual(await pending('discover'), { catalog: true, id: catalog.id });
    await settle([{ id: 'gpt-5.1', name: 'GPT-5.1', inputModalities: ['text', 'image'], contextWindow: 400000, maxTokens: 128000 },
      { id: 'gpt-5-mini', name: 'GPT-5 mini' }]);
    await picker.getByRole('checkbox', { name: 'gpt-5-mini' }).uncheck();
    await picker.getByRole('button', { name: '更新列表' }).click();
    await picker.waitFor({ state: 'hidden' });
    await form.getByRole('button', { name: '模型高级参数 gpt-5.1', exact: true }).click();
    assert.equal(await form.getByRole('group', { name: '推理等级 1' }).isVisible(), false);
    await page.screenshot({ path: join(artifacts, 'installed-provider.png') });
    await save.click();
    const catalogDraft = await pending('save');
    assert.equal(catalogDraft.id, catalog.id);
    assert.equal(catalogDraft.catalog, true);
    assert.equal(catalogDraft.expectedRevision, 50);
    assert.equal(catalogDraft.baseUrl, 'https://override.fixture.invalid/v1');
    assert.equal(catalogDraft.apiKey, 'sk-directory-fixture');
    assert.equal(catalogDraft.protocol, undefined);
    assert.deepEqual(catalogDraft.models.map(({ id, contextWindow, inputModalities, reasoningEfforts }) => (
      { id, contextWindow, inputModalities, reasoningEfforts }
    )), [{ id: 'gpt-5.1', contextWindow: 400000, inputModalities: ['text', 'image'], reasoningEfforts: undefined }]);
    const activated = { ...catalog, baseUrl: catalogDraft.baseUrl, keyConfigured: true, available: true,
      canClearKey: true, removable: true, models: catalogDraft.models, catalog: { ...catalog.catalog, modelsOverridden: true } };
    await settle({ id: catalog.id, providers: [activated], catalogProviders: [codex], revision: 51 });
    await form.getByRole('status').getByText('模型服务已保存').waitFor();
    await form.getByRole('button', { name: '恢复默认模型', exact: true }).click();
    await form.getByText('使用完整的内置模型目录。').waitFor();
    await save.click();
    const reset = await pending('save');
    assert.equal(reset.models, null);
    assert.equal(reset.expectedRevision, 51);
    assert.equal(reset.apiKey, undefined);
    await settle({ id: catalog.id, providers: [{ ...activated, models: [], catalog: catalog.catalog }], catalogProviders: [codex], revision: 52 });
    await form.getByRole('status').getByText('模型服务已保存').waitFor();
    await remove.click();
    await form.getByRole('alertdialog', { name: '删除服务商 openai' }).getByRole('button', { name: '确认删除' }).click();
    assert.deepEqual(await pending('remove'), { id: catalog.id, catalog: true, expectedRevision: 52 });
    await settle({ id: catalog.id, providers: [], catalogProviders: [catalog, codex], revision: 53 });
    await form.getByRole('status').getByText('模型服务已删除').waitFor();
    await search.fill('openai');
    await directory.getByRole('button', { name: 'openai', exact: true }).waitFor();
  } else if (mode === 'provider-settings') {
    page.setDefaultTimeout(10000);
    await page.getByRole('button', { name: '配置模型服务', exact: true }).click();
    const form = page.getByRole('region', { name: '模型服务', exact: true });
    const heading = form.getByRole('heading', { name: '模型服务', exact: true });
    const name = form.getByLabel('服务商名称', { exact: true });
    const urlField = form.getByLabel('API 地址', { exact: true });
    const key = form.locator('[data-field="key"]');
    const sync = form.getByRole('button', { name: '同步模型', exact: true });
    const save = form.getByRole('button', { name: '保存服务商', exact: true });
    const status = form.locator('[data-part="feedback"]');
    const configured = form.getByRole('group', { name: '已添加的模型' });
    const modelIds = () => form.locator('[data-model-field="id"]').evaluateAll(inputs => inputs.map(input => input.value));
    const picker = form.getByRole('dialog', { name: '选择模型', exact: true });
    const candidateSearch = picker.getByRole('searchbox', { name: '搜索模型' });
    const updateList = picker.getByRole('button', { name: '更新列表' });
    const candidate = id => picker.getByRole('checkbox', { name: id, exact: true });
    const nav = page.locator('#composer-submit-accessibility');
    const openProviders = async () => {
      await nav.getByRole('button', { name: '更多设置', exact: true }).dispatchEvent('click');
      await nav.getByRole('button', { name: '设置', exact: true }).dispatchEvent('click');
      await page.locator('#settings-shell').getByRole('button', { name: '模型服务', exact: true }).click();
      await form.waitFor();
    };
    const calls = () => page.evaluate(() => window.__verification.calls.filter(call => call.name === 'model_providers').map(call => call.args));
    const waitRequest = async action => {
      await page.waitForFunction(() => typeof window.__verification.settleProviderSettings === 'function');
      const request = (await calls()).at(-1);
      assert.equal(request.action, action);
      return request.draft;
    };
    const settle = (answer, error = null) => page.evaluate(({ answer, error }) => window.__verification.settleProviderSettings(answer, error), { answer, error });
    const focused = locator => locator.evaluate(element => element === document.activeElement);
    const discover = async models => {
      await sync.click();
      await waitRequest('discover');
      await settle(models);
      await picker.waitFor();
    };

    await heading.waitFor();
    await page.waitForFunction(() => document.activeElement?.matches('#settings-shell [aria-current]'));
    await waitRequest('list');
    await settle(null, '服务商列表暂不可用');
    await status.getByText('服务商列表暂不可用').waitFor();
    await form.getByRole('button', { name: '重新加载', exact: true }).click();
    await waitRequest('list');
    const backoffSpec = {
      initialDelayMs: { min: Number.MIN_VALUE, max: 2147483647, value: 500 },
      maxDelayMs: { min: Number.MIN_VALUE, max: 2147483647, value: 10000 },
      jitterRatio: { min: 0, max: 1, value: 0.1 },
    };
    const defaultCodes = ['EMPTY_RESPONSE', 'RATE_LIMIT', 'SERVER', 'TIMEOUT', 'TRANSPORT'];
    await settle({ providers: [], retryPolicySpec: {
      maxRetries: { min: 0, max: Number.MAX_SAFE_INTEGER, value: 5 },
      normalBackoff: backoffSpec, alwaysBackoff: backoffSpec, defaultCodes,
      choices: [...defaultCodes, 'MALFORMED_RESPONSE', 'STREAM_CLOSED', 'QUOTA'],
    } });
    await form.getByRole('button', { name: 'OpenAI', exact: true }).click();
    await page.waitForFunction(() => document.querySelector('#model-provider-form [data-field="name"]')?.value === 'OpenAI');
    await form.getByRole('button', { name: '添加服务商', exact: true }).click();
    await page.waitForFunction(() => document.querySelector('#model-provider-form [data-field="name"]')?.value === '');
    await name.fill('Fixture gateway');
    await urlField.fill('invalid-address');
    await form.getByRole('alert').getByText(/有效的 HTTP 或 HTTPS 基础地址/).waitFor();
    assert.equal(await sync.isDisabled(), true);
    assert.equal(await save.isDisabled(), true);
    await urlField.fill('https://fixture.invalid/v1');
    await form.getByLabel('接口协议', { exact: true }).selectOption('openai-responses');
    await key.evaluate(input => { input.value = 'sk-providers-fixture'; });
    const reveal = form.getByRole('button', { name: '显示服务商密钥' });
    await reveal.waitFor();
    assert.equal(await key.getAttribute('type'), 'password');
    await reveal.click();
    assert.equal(await key.getAttribute('type'), 'text');
    await form.getByRole('button', { name: '隐藏服务商密钥' }).click();
    assert.equal(await key.getAttribute('type'), 'password');

    await sync.click();
    const probe = await waitRequest('discover');
    assert.equal(probe.baseUrl, 'https://fixture.invalid/v1');
    assert.equal(probe.protocol, 'openai-responses');
    assert.equal(probe.apiKey, 'sk-providers-fixture');
    await settle(null, '服务商模型列表请求失败');
    await status.getByText('服务商模型列表请求失败').waitFor();
    assert.ok(await focused(sync));
    await sync.click();
    await waitRequest('discover');
    await settle([]);
    await status.getByText('该服务商没有列出任何模型，请手动添加。').waitFor();
    assert.equal(await picker.count(), 0);
    const discovered = { id: 'vision-fixture', name: 'Vision fixture', contextWindow: 10000, inputModalities: ['text', 'image'] };
    const textModel = { id: 'text-fixture', name: 'Plain text gateway', maxTokens: 24000, inputModalities: ['text'] };
    const listing = [{ ...discovered, contextWindow: 1, name: 'Obsolete name' }, textModel, discovered];
    await discover(listing);
    assert.ok(await focused(candidateSearch));
    assert.equal(await candidate(discovered.id).count(), 1);
    assert.equal(await candidate(discovered.id).isChecked(), true);
    assert.equal(await candidate(textModel.id).isChecked(), true);
    assert.equal(await picker.getByText(discovered.id, { exact: true }).getAttribute('title'), discovered.name);
    assert.equal(await page.getByRole('button', { name: '通用设置', exact: true }).count(), 0);
    const closePicker = picker.getByRole('button', { name: '关闭模型选择' });
    await closePicker.focus();
    await page.keyboard.press('Shift+Tab');
    assert.ok(await focused(updateList));
    await page.keyboard.press('Tab');
    assert.ok(await focused(closePicker));
    await candidateSearch.fill('unmatched-model-name');
    await picker.getByRole('status').getByText('没有匹配的模型。').waitFor();
    assert.equal(await picker.getByRole('button', { name: '全选', exact: true }).isDisabled(), true);
    await candidateSearch.fill('pln');
    await candidate(textModel.id).waitFor();
    assert.equal(await candidate(discovered.id).count(), 0);
    await picker.getByRole('button', { name: '取消全选', exact: true }).click();
    await candidateSearch.fill('');
    await candidate(discovered.id).waitFor();
    assert.equal(await candidate(discovered.id).isChecked(), true);
    assert.equal(await candidate(textModel.id).isChecked(), false);
    await page.screenshot({ path: join(artifacts, 'provider-model-picker.png') });
    await page.keyboard.press('Escape');
    await picker.waitFor({ state: 'hidden' });
    assert.ok(await focused(sync));
    assert.deepEqual(await modelIds(), []);

    await discover(listing);
    assert.equal(await candidateSearch.inputValue(), '');
    assert.equal(await candidate(textModel.id).isChecked(), true);
    await candidate(textModel.id).uncheck();
    await updateList.click();
    await picker.waitFor({ state: 'hidden' });
    assert.ok(await focused(sync));
    assert.deepEqual(await modelIds(), [discovered.id]);
    assert.equal((await calls()).some(call => call.action === 'save'), false);
    await discover(listing);
    assert.equal(await candidate(discovered.id).isChecked(), true);
    assert.equal(await candidate(textModel.id).isChecked(), false);
    await candidate(discovered.id).uncheck();
    await picker.getByRole('alert').getByText('请至少保留一个模型。').waitFor();
    assert.equal(await updateList.isDisabled(), true);
    await picker.getByRole('button', { name: '取消', exact: true }).click();
    await picker.waitFor({ state: 'hidden' });
    assert.deepEqual(await modelIds(), [discovered.id]);
    await form.getByRole('button', { name: '手动添加', exact: true }).click();
    const manual = configured.getByLabel('模型 ID 2', { exact: true });
    await manual.waitFor();
    assert.ok(await focused(manual));
    await manual.fill(' vision-fixture ');
    await configured.getByRole('alert').getByText('模型 ID 必须唯一。').waitFor();
    assert.equal(await save.isDisabled(), true);
    await manual.fill('manual-fixture');
    await form.getByRole('button', { name: '移除 manual-fixture', exact: true }).waitFor();
    await configured.getByRole('button', { name: '模型高级参数 vision-fixture', exact: true }).click();
    const contextWindow = configured.getByLabel('上下文窗口 1', { exact: true });
    const maxTokens = configured.getByLabel('最大输出令牌数 1', { exact: true });
    const modelName = configured.getByLabel('显示名称 1', { exact: true });
    assert.equal(await contextWindow.inputValue(), '10K');
    await contextWindow.fill('0.5');
    await configured.getByRole('alert').getByText(/上下文窗口/).waitFor();
    assert.equal(await save.isDisabled(), true);
    await contextWindow.fill('2.3M');
    await maxTokens.fill('32K');
    await modelName.fill('Edited vision');
    await configured.getByRole('checkbox', { name: '文字', exact: true }).uncheck();
    const imageOnly = configured.getByRole('checkbox', { name: '图片', exact: true });
    await imageOnly.and(configured.locator(':disabled')).waitFor();
    assert.equal(await imageOnly.isChecked(), true);
    await page.setViewportSize({ width: 760, height: 640 });
    const many = Array.from({ length: 48 }, (_, index) => ({ id: `gateway-model-${index}` }));
    await discover([...listing, ...many]);
    await page.waitForFunction(() => {
      const dialog = document.querySelector('.provider-picker-dialog');
      const results = dialog.querySelector('.provider-picker-results');
      const footer = dialog.querySelector('footer').getBoundingClientRect();
      const bounds = dialog.getBoundingClientRect();
      return bounds.top >= 0 && bounds.bottom <= innerHeight && footer.bottom <= bounds.bottom
        && footer.right <= innerWidth && results.clientHeight > 0 && results.scrollHeight > results.clientHeight;
    });
    await page.screenshot({ path: join(artifacts, 'provider-model-picker-compact.png') });
    await updateList.click();
    await picker.waitFor({ state: 'hidden' });
    await page.setViewportSize({ width: 1440, height: 1100 });
    await page.waitForFunction(() => document.getElementById('model-provider-form').getBoundingClientRect().width === 564);
    assert.equal(await contextWindow.inputValue(), '2.3M');
    assert.equal(await modelName.inputValue(), 'Edited vision');
    assert.equal(await imageOnly.isChecked(), true);
    assert.deepEqual(await modelIds(), [discovered.id, 'manual-fixture']);
    await page.screenshot({ path: join(artifacts, 'provider-model-parameters.png') });
    const order = configured.getByRole('button', { name: '排序模型 manual-fixture', exact: true });
    const waitOrder = expected => page.waitForFunction(expected => {
      const ids = [...document.querySelectorAll('[data-model-field="id"]')].map(input => input.value);
      return ids.every((id, index) => expected[index] === id) && ids.length === expected.length;
    }, expected);
    await order.press('ArrowUp');
    await waitOrder(['manual-fixture', 'vision-fixture']);
    assert.ok(await focused(order));
    const visionRow = configured.locator('.provider-model-entry').filter({
      has: page.getByRole('checkbox', { name: '图片', exact: true }),
    });
    await visionRow.scrollIntoViewIfNeeded();
    const orderBounds = await order.boundingBox();
    const visionBounds = await visionRow.boundingBox();
    const drop = { x: visionBounds.x + 6, y: visionBounds.y + visionBounds.height - 6 };
    await page.mouse.move(orderBounds.x + orderBounds.width / 2, orderBounds.y + orderBounds.height / 2);
    await page.mouse.down();
    try {
      await page.mouse.move(drop.x, drop.y, { steps: 8 });
      await page.mouse.move(drop.x, drop.y - 2);
      await visionRow.and(configured.locator('[data-drop="after"]')).waitFor({ state: 'attached' });
    } finally {
      await page.mouse.up();
    }
    await waitOrder(['vision-fixture', 'manual-fixture']);
    const reasoning = configured.getByRole('group', { name: '推理等级 1', exact: true });
    const reasoningToggle = reasoning.getByRole('switch', { name: '推理等级 1' });
    const reasoningValue = level => reasoning.getByRole('textbox', { name: `${level} API 参数值`, exact: true });
    const reasoningLevel = position => reasoning.getByRole('combobox', { name: `档位 ${position}`, exact: true });
    const removeLevel = level => reasoning.getByRole('button', { name: `删除档位 ${level}`, exact: true });
    const addLevel = reasoning.getByRole('button', { name: '添加档位' });
    await reasoningToggle.check();
    await reasoningValue('high').waitFor();
    assert.equal(await reasoningValue('off').isDisabled(), true);
    assert.equal(await reasoningValue('low').inputValue(), 'low');
    assert.equal(await reasoningValue('medium').inputValue(), 'medium');
    await removeLevel('low').click();
    await reasoning.evaluate(group => {
      group.querySelector('button[aria-label="删除档位 medium"]').click();
      group.querySelector('button[aria-label="删除档位 high"]').click();
    });
    await configured.getByRole('alert').getByText('至少添加一个 Off 以外的档位，或关闭推理等级。').waitFor();
    assert.equal(await save.isDisabled(), true);
    await addLevel.click();
    await reasoningValue('minimal').waitFor();
    await reasoningLevel(2).focus();
    await reasoningLevel(2).selectOption('high');
    await reasoningValue('high').fill(' vendor-strong ');
    await reasoningLevel(2).focus();
    await reasoningLevel(2).selectOption('xhigh');
    await reasoningValue('xhigh').waitFor();
    assert.ok(await focused(reasoningLevel(2)));
    assert.equal(await reasoningValue('xhigh').inputValue(), 'vendor-strong');
    assert.equal(await reasoningLevel(2).locator('option[value="off"]').count(), 0);
    await addLevel.click();
    await reasoningValue('max').waitFor();
    await reasoningValue('max').fill('');
    assert.equal(await reasoningValue('max').inputValue(), '');
    await reasoningLevel(3).focus();
    assert.equal(await reasoningValue('max').inputValue(), 'max');
    const efforts = [['off', null], ['xhigh', 'vendor-strong'], ['max', 'max']];
    await page.screenshot({ path: join(artifacts, 'provider-model-reasoning.png') });
    const retry = form.getByRole('region', { name: '重试策略', exact: true });
    const retryMode = retry.getByRole('combobox', { name: '重试策略', exact: true });
    const retryCount = retry.getByRole('textbox', { name: '最大重试次数' });
    const initialDelay = retry.getByRole('textbox', { name: '初始延迟 (ms)' });
    const maxDelay = retry.getByRole('textbox', { name: '最大延迟 (ms)' });
    const jitter = retry.getByRole('textbox', { name: '抖动比例' });
    const retryIssue = retry.getByRole('alert');
    assert.equal(await retryMode.inputValue(), '');
    assert.equal(await retryCount.count(), 0);
    await retryMode.selectOption('normal');
    await retryCount.fill('1.5');
    await retryIssue.getByText('请输入整数。').waitFor();
    assert.equal(await save.isDisabled(), true);
    assert.equal(await retryCount.getAttribute('placeholder'), '5');
    await retryCount.fill('1');
    const extraCode = retry.getByRole('textbox', { name: '自定义错误码' });
    await extraCode.fill('UPSTREAM_BUSY');
    await extraCode.press('Enter');
    const extraChoice = retry.getByRole('checkbox', { name: 'UPSTREAM_BUSY' });
    await extraChoice.waitFor();
    assert.equal(await extraChoice.isChecked(), true);
    assert.equal(await extraCode.inputValue(), '');
    await extraCode.fill(' UPSTREAM_BUSY ');
    await extraCode.press('Enter');
    assert.equal(await extraCode.inputValue(), '');
    assert.equal(await extraChoice.count(), 1);
    assert.equal((await calls()).some(call => call.action === 'save'), false);
    await extraChoice.uncheck();
    assert.equal(await extraChoice.count(), 1);
    await extraChoice.check();
    await retry.getByText('退避', { exact: true }).click();
    assert.equal(await maxDelay.getAttribute('placeholder'), '10000');
    await initialDelay.fill('12000');
    await retryIssue.getByText('初始延迟不能大于最大延迟。').waitFor();
    await initialDelay.fill('0');
    await retryIssue.getByText('必须大于 0。').waitFor();
    await initialDelay.fill('20');
    await maxDelay.fill('120');
    await jitter.fill('2');
    await retryIssue.getByText('不能大于 1。').waitFor();
    await jitter.fill('0.25');
    await page.screenshot({ path: join(artifacts, 'provider-retry-policy.png') });
    await save.click();
    const draft = await waitRequest('save');
    assert.equal(draft.apiKey, 'sk-providers-fixture');
    assert.deepEqual(draft.models.map(model => model.id), ['vision-fixture', 'manual-fixture']);
    assert.deepEqual(draft.models[0], {
      id: 'vision-fixture', sourceId: null, name: 'Edited vision', contextWindow: 2300000,
      maxTokens: 32000, inputModalities: ['image'], reasoningEfforts: efforts,
    });
    assert.deepEqual(draft.retryPolicy, {
      mode: 'normal', maxRetries: 1, retryableCodes: [...defaultCodes, 'UPSTREAM_BUSY'],
      backoff: { initialDelayMs: 20, maxDelayMs: 120, jitterRatio: 0.25 },
    });
    assert.equal(await form.getByRole('button', { name: '保存中…' }).isDisabled(), true);
    await settle(null, '服务商保存失败');
    await status.getByText('服务商保存失败').waitFor();
    assert.equal(await key.inputValue(), 'sk-providers-fixture');
    assert.equal(await name.inputValue(), 'Fixture gateway');
    assert.equal(await contextWindow.inputValue(), '2.3M');
    assert.equal(await reasoningValue('xhigh').inputValue(), 'vendor-strong');
    assert.equal(await jitter.inputValue(), '0.25');
    assert.deepEqual(await modelIds(), ['vision-fixture', 'manual-fixture']);
    assert.ok(await focused(save));
    await page.screenshot({ path: join(artifacts, 'provider-save-retry.png') });
    await save.click();
    assert.deepEqual(await waitRequest('save'), draft);
    const inheritedRetryPolicy = { mode: 'normal', backoff: { maxDelayMs: 408 } };
    await settle({ id: 'fixture-gateway', providers: [{ ...draft, id: 'fixture-gateway', keyConfigured: true, available: true, inheritedRetryPolicy }] });
    await status.getByText('模型服务已保存').waitFor();
    assert.equal(await key.inputValue(), '');
    await configured.getByRole('button', { name: '模型高级参数 vision-fixture', exact: true }).click();
    assert.equal(await reasoningValue('off').inputValue(), '');
    assert.equal(await reasoningValue('xhigh').inputValue(), 'vendor-strong');
    await reasoningToggle.uncheck();
    await retryMode.selectOption('always');
    await retry.getByText('失败的请求会持续重试，直到成功或本轮被停止。').waitFor();
    assert.equal(await retryCount.count(), 0);
    assert.equal(await extraChoice.count(), 0);
    assert.equal(await initialDelay.inputValue(), '20');
    assert.equal(await maxDelay.getAttribute('placeholder'), '408');
    await maxDelay.fill('');
    await configured.getByLabel('模型 ID 1', { exact: true }).fill('renamed-fixture');
    await modelName.fill('');
    await contextWindow.fill('');
    await maxTokens.fill('');
    await save.click();
    const reset = await waitRequest('save');
    assert.deepEqual(reset.models[0], {
      id: 'renamed-fixture', sourceId: 'vision-fixture', name: null, contextWindow: null,
      maxTokens: null, inputModalities: ['image'], reasoningEfforts: null,
    });
    assert.deepEqual(reset.retryPolicy, { mode: 'always', backoff: { initialDelayMs: 20, jitterRatio: 0.25 } });
    await settle({ id: 'fixture-gateway', providers: [{ ...reset, id: 'fixture-gateway', keyConfigured: true, available: true, inheritedRetryPolicy }] });
    await status.getByText('模型服务已保存').waitFor();
    await form.getByRole('button', { name: '清除已保存密钥', exact: true }).click();
    await sync.and(form.locator(':disabled')).waitFor();
    await form.getByRole('button', { name: '保留已保存密钥', exact: true }).click();
    assert.equal(await key.inputValue(), '');

    await sync.click();
    await waitRequest('discover');
    await page.locator('#settings-shell').getByRole('button', { name: '关闭设置', exact: true }).click();
    await form.waitFor({ state: 'hidden' });
    await openProviders();
    await settle(listing);
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(await picker.count(), 0);

    const renamedModel = { ...discovered, id: 'renamed-fixture' };
    await discover([renamedModel, textModel]);
    assert.equal(await candidate(renamedModel.id).isChecked(), true);
    await candidate(renamedModel.id).uncheck();
    await candidate(textModel.id).check();
    await updateList.click();
    await picker.waitFor({ state: 'hidden' });
    assert.deepEqual(await modelIds(), ['manual-fixture', textModel.id]);
    await retryMode.selectOption('');
    await save.click();
    const synced = await waitRequest('save');
    assert.equal(synced.retryPolicy, null);
    assert.equal(synced.models[1].maxTokens, 24000);
    assert.deepEqual(synced.models[1].inputModalities, ['text']);
    await settle({ id: 'fixture-gateway', providers: [{ ...synced, id: 'fixture-gateway', keyConfigured: true, available: true }] });
    await status.getByText('模型服务已保存').waitFor();

    const ask = form.getByRole('button', { name: '删除服务商', exact: true });
    await ask.click();
    const confirmation = form.getByRole('alertdialog', { name: '删除服务商 Fixture gateway' });
    const cancel = confirmation.getByRole('button', { name: '取消', exact: true });
    await cancel.waitFor();
    assert.ok(await focused(cancel));
    assert.equal(await page.getByRole('button', { name: '通用设置', exact: true }).count(), 0);
    await page.keyboard.press('Escape');
    await confirmation.waitFor({ state: 'hidden' });
    assert.ok(await focused(ask));
    await ask.click();
    const confirm = confirmation.getByRole('button', { name: '确认删除', exact: true });
    await confirm.click();
    assert.deepEqual(await waitRequest('remove'), { id: 'fixture-gateway' });
    await settle(null, '删除操作失败');
    await confirmation.getByRole('alert').getByText('删除操作失败').waitFor();
    await confirm.click();
    assert.deepEqual(await waitRequest('remove'), { id: 'fixture-gateway' });
    await settle({ id: 'fixture-gateway', providers: [] });
    await confirmation.waitFor({ state: 'hidden' });
    await status.getByText('模型服务已删除').waitFor();
    await page.locator('#settings-shell').getByRole('button', { name: '关闭设置', exact: true }).click();
    await form.waitFor({ state: 'hidden' });
    await openProviders();
    await page.waitForFunction(() => document.activeElement?.matches('#settings-shell [aria-current]'));
    await page.keyboard.press('Escape');
    await form.waitFor({ state: 'hidden' });
  } else if (mode === 'welcome') {
    page.setDefaultTimeout(10000);
    const form = page.getByRole('dialog', { name: '添加一个 API Key 开始使用' });
    const key = form.getByLabel('API 密钥', { exact: true });
    const save = form.getByRole('button', { name: '保存并继续' });
    const providers = form.getByRole('button', { name: '配置模型服务' });
    const later = form.getByRole('button', { name: '稍后配置' });
    const reveal = form.getByRole('button', { name: '显示密钥' });
    const callArgs = name => page.evaluate(name => window.__verification.calls.filter(call => call.name === name).map(call => call.args), name);
    const waitForAuth = () => page.waitForFunction(() => typeof window.__verification.settleWelcomeAuth === 'function');
    const settleAuth = () => page.evaluate(() => window.__verification.settleWelcomeAuth());
    const waitForSave = () => page.waitForFunction(() => typeof window.__verification.settleWelcomeSave === 'function');
    const settleSave = failure => page.evaluate(failure => window.__verification.settleWelcomeSave(failure), failure);
    const focusIs = async element => page.waitForFunction(
      ref => document.activeElement === ref, await element.elementHandle());
    const go = scenario => page.goto(`${url}?scenario=${scenario}`);

    await waitForAuth();
    await page.getByRole('status').getByText('正在读取模型配置…').waitFor();
    assert.equal(await form.count(), 0);
    await settleAuth();
    await focusIs(key);
    assert.equal(await key.getAttribute('type'), 'password');
    assert.equal(await save.isDisabled(), true);
    await page.keyboard.press('Shift+Tab');
    await focusIs(later);
    await page.keyboard.press('Tab');
    await focusIs(key);
    await page.keyboard.press('Escape');
    assert.ok(await form.isVisible());
    assert.equal(await page.locator('#gpui-text-input').isVisible(), false);
    await key.evaluate(field => { field.value = 'sk-fixture-without-an-input-event'; });
    await reveal.waitFor();
    assert.equal(await save.isDisabled(), false);

    await key.fill('   ');
    assert.equal(await save.isDisabled(), true);
    await key.fill('DEEPSEEK_API_KEY=test-fixture');
    await save.click();
    await form.getByRole('alert').getByText(/环境变量赋值/).waitFor();
    assert.deepEqual(await callArgs('save_api_key'), []);
    await key.fill('  sk-welcome-regression-fixture  ');
    await reveal.click();
    assert.equal(await key.getAttribute('type'), 'text');
    await key.fill('  sk-welcome-regression-retry  ');
    assert.equal(await key.getAttribute('type'), 'password');
    assert.equal(await page.locator('#gpui-text-input').inputValue(), '');
    await key.press('Enter');
    await waitForSave();
    await form.getByRole('button', { name: '保存中…' }).waitFor();
    assert.equal(await later.isDisabled(), true);
    assert.equal(await providers.isDisabled(), true);
    await key.press('Enter');
    assert.deepEqual(await callArgs('save_api_key'), [{ value: 'sk-welcome-regression-retry' }]);
    await settleSave('Fixture credential write failed');
    await form.getByRole('alert').getByText('Fixture credential write failed').waitFor();
    assert.equal(await key.inputValue(), '  sk-welcome-regression-retry  ');
    await focusIs(key);
    await page.screenshot({ path: join(artifacts, 'welcome-save-failure.png') });
    await key.press('Enter');
    await waitForSave();
    await settleSave(null);
    await form.waitFor({ state: 'hidden' });
    assert.equal(await page.locator('#welcome-setup input').count(), 0);
    await page.waitForFunction(() => document.activeElement?.id === 'gpui-text-input' &&
      document.activeElement.hasAttribute('aria-label'));

    await go('later');
    await waitForAuth();
    await settleAuth();
    await later.click();
    await form.waitFor({ state: 'hidden' });
    await page.getByRole('button', { name: '调用指令', exact: true }).waitFor();
    assert.deepEqual(await callArgs('save_api_key'), []);
    assert.deepEqual(await callArgs('model_providers'), []);

    await go('providers');
    await waitForAuth();
    await settleAuth();
    await providers.click();
    await form.waitFor({ state: 'hidden' });
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'model_providers' && call.args.action === 'list'));
    assert.deepEqual(await callArgs('save_api_key'), []);

    await go('configured');
    await page.getByRole('button', { name: '调用指令', exact: true }).waitFor();
    assert.equal(await form.count(), 0);
    await go('compatible');
    await waitForAuth();
    await page.getByRole('button', { name: '调用指令', exact: true }).waitFor();
    await settleAuth();
    assert.equal(await form.count(), 0);
    assert.deepEqual(await callArgs('save_api_key'), []);
  } else if (mode === 'workspace-startup') {
    const calls = (name) => page.evaluate(name => window.__verification.calls.filter(call => call.name === name).map(call => call.args), name);
    const waitForCall = name => page.waitForFunction(name => window.__verification.calls.some(call => call.name === name), name, { timeout: 5000 }).catch(async error => {
      console.error('STARTUP_SCENARIO', page.url(), name, await calls('workspace_registry'), await calls('pick_workspace_directory'));
      throw error;
    });
    const waitForDefault = () => page.waitForFunction(() => Boolean(window.__verification.settleDefaultWorkspace), null, { timeout: 5000 }).catch(async error => {
      console.error('WORKSPACE_STARTUP_CALLS', await page.evaluate(() => window.__verification.calls.map(call => call.name)));
      throw error;
    });
    const begin = async scenario => {
      await page.goto(`${url}?scenario=${scenario}`);
      await page.waitForFunction(() => document.querySelector('canvas') && !document.getElementById('boot'), null, { timeout: 60000 });
      await page.waitForFunction(() => window.__verification.calls.filter(call => call.name === 'list_local_sessions').length >= 3, null, { timeout: 5000 });
    };
    const settle = result => page.evaluate(result => window.__verification.settleDefaultWorkspace(result), result);
    const snapshot = path => ({ available: true, sequence: 2, path, workspaceId: path ? defaultWorkspace.id : null, items: path ? [defaultWorkspace] : [], pinned: [], archived: [] });
    await waitForDefault();
    assert.deepEqual(await calls('new_session'), [], 'creating the default workspace must precede creating its session');
    await settle(snapshot(defaultPath));
    await waitForCall('new_session');
    assert.deepEqual(await calls('new_session'), [{ cwd: defaultPath, workspaceId: defaultWorkspace.id }]);
    assert.deepEqual(await calls('prompt'), []);
    await page.waitForTimeout(250);
    await page.screenshot({ path: join(artifacts, 'first-use-workspace.png') });

    await begin('failure');
    await waitForDefault();
    await settle({ error: 'Documents unavailable' });
    await page.waitForTimeout(270);
    await page.screenshot({ path: join(artifacts, 'default-workspace-failure.png') });
    await page.mouse.click(140, 122);
    assert.deepEqual(await calls('new_session'), []);
    assert.deepEqual(await calls('workspace_registry'), [{ operation: { action: 'initialize-default' } }]);
    await page.mouse.click(551, 500);
    await waitForCall('new_session');
    assert.deepEqual(await calls('new_session'), [{ cwd: canonicalDirectory, workspaceId: 'host-picked' }]);

    await begin('history');
    await page.waitForTimeout(250);
    await page.mouse.click(140, 122);
    assert.deepEqual(await calls('workspace_registry'), []);
    assert.deepEqual(await calls('new_session'), []);

    await begin('removed');
    await waitForDefault();
    await settle(snapshot(null));
    await page.waitForTimeout(250);
    await page.mouse.click(140, 122);
    assert.deepEqual(await calls('new_session'), []);

    await begin('late');
    await waitForDefault();
    await page.keyboard.press('ControlOrMeta+o');
    await waitForCall('new_session');
    await settle(snapshot(defaultPath));
    await page.waitForTimeout(270);
    assert.deepEqual(await calls('new_session'), [{ cwd: canonicalDirectory, workspaceId: 'host-picked' }]);

    await begin('recent');
    await waitForCall('new_session');
    assert.deepEqual(await calls('new_session'), [{ cwd, workspaceId: 'host-parent' }]);
    assert.deepEqual(await calls('workspace_registry'), []);

    await begin('restore');
    await waitForCall('resume_session');
    assert.deepEqual(await calls('resume_session'), [{ sessionId, cwd }]);
    assert.deepEqual(await calls('new_session'), []);
  } else if (mode === 'workspace-search') {
    const controls = page.locator('#workspace-accessibility-search');
    const trigger = controls.getByRole('button', { name: '搜索会话', exact: true });
    const field = controls.getByRole('textbox', { name: '搜索会话名称' });
    const status = controls.getByRole('status');
    const search = page.getByRole('tree', { name: '搜索结果', exact: true });
    const rows = search.locator('[role="treeitem"]');
    const row = name => search.getByRole('treeitem', { name, exact: true });
    const labels = () => rows.evaluateAll(elements => elements.map(element => element.getAttribute('aria-label')));
    const searchCalls = () => page.evaluate(() => window.__verification.calls
      .filter(call => call.name === 'search_session_history').map(call => call.args));
    const requested = query => page.waitForFunction(query => window.__verification.calls
      .some(call => call.name === 'search_session_history' && call.args.query === query), query, { timeout: 5000 });
    const finish = (query, items = [], hasMore = false) => page.evaluate(
      ({ query, items, hasMore }) => window.__verification.finishSearch(query, items, hasMore), { query, items, hasMore });
    const focus = (name, role) => page.waitForFunction(({ name, role }) => {
      const element = document.activeElement;
      return element?.closest('#workspace-accessibility-search') && element.tagName === role
        && (element.getAttribute('aria-label') ?? element.textContent) === name;
    }, { name, role }, { timeout: 5000 });
    const settled = (text = '') => page.waitForFunction(text =>
      document.querySelector('#workspace-accessibility-search [role=status]')?.textContent === text, text, { timeout: 5000 });

    await trigger.dispatchEvent('click');
    await focus('搜索会话名称', 'INPUT');
    assert.equal(await page.getByRole('tree', { name: '工作区和会话' })
      .getByRole('treeitem', { name: 'Review workflow', exact: true }).count(), 0,
      'the result is beyond the browsing watermark');
    await field.fill('\uFEFF  index  \uFEFF');
    await row('Index launch plan').waitFor({ state: 'attached' });
    assert.deepEqual((await labels()).slice(0, 3), ['Index fixture 0', 'Index launch plan', 'Index fixture 22']);
    assert.equal(await rows.count(), 20);
    assert.equal(await row('Review index notes').count(), 0);
    await requested('index');
    assert.match(await status.textContent(), /正在搜索会话历史/);
    await page.screenshot({ path: join(artifacts, 'pending-search.png') });
    await finish('index');
    await settled('仅显示前 20 条结果，请缩小搜索范围。');
    await field.fill('rst');
    await row('Registry state transition').waitFor({ state: 'attached' });
    assert.deepEqual(await labels(), ['Registry state transition', 'Run smoke tests'],
      'the stronger official fuzzy score wins over recency');
    await field.fill('か');
    await row('か').waitFor({ state: 'attached' });
    assert.deepEqual(await labels(), ['か'], 'unvoiced kana does not match voiced kana');
    await field.fill('が');
    await row('が').waitFor({ state: 'attached' });
    assert.deepEqual(await labels(), ['が']);

    await field.fill('rvw host 41abc');
    await row('Review workflow').waitFor({ state: 'attached' });
    assert.deepEqual(await labels(), ['Review workflow']);
    assert.match(await row('Review workflow').getAttribute('aria-description'), /Host parent/);
    await requested('rvw host 41abc');
    const renameWorkspace = (sequence, title) => page.evaluate(({ sequence, title, hostWorkspaces }) => {
      const items = hostWorkspaces.map(workspace => workspace.id === 'host-parent' ? { ...workspace, title } : workspace);
      window.__verification.emit({
        kind: 'workspace_registry', available: true, sequence, items,
        pinned: ['index-suffix'], archived: ['archived-match'],
      });
    }, { sequence, title, hostWorkspaces });
    await renameWorkspace(2, 'Renamed account');
    await row('Review workflow').waitFor({ state: 'detached' });
    assert.equal(await field.inputValue(), 'rvw host 41abc');
    await renameWorkspace(3, 'Host parent');
    await row('Review workflow').waitFor({ state: 'attached' });

    await field.fill('needle');
    await row('Needle index').waitFor({ state: 'attached' });
    assert.deepEqual(await labels(), ['Needle index'], 'hidden archived rows and durable blank sessions stay out');
    await requested('needle');
    await finish('needle', [
      { sessionId: 'content-one', snippet: 'First current message hit for needle' },
      { sessionId: 'archived-match', snippet: 'Archived match' },
      { sessionId: 'durable-blank', snippet: 'Blank hit' },
      { sessionId: 'needle-title', snippet: 'Deduplicated current message snippet' },
      { sessionId: 'content-one', snippet: 'Duplicate hit' },
      { sessionId: 'missing-session', snippet: 'Unknown hit' },
      { sessionId: 'content-two', snippet: 'Second current message hit' },
    ], true);
    await settled('仅显示前 20 条结果，请缩小搜索范围。');
    assert.deepEqual(await labels(), ['Needle index', 'Content one', 'Content two']);
    assert.match(await row('Needle index').getAttribute('aria-description'), /Deduplicated current message snippet/);
    assert.match(await row('Content one').getAttribute('aria-description'), /First current message/);
    await finish('rvw host 41abc', [{ sessionId: 'search-41abc', snippet: 'Stale response' }]);
    await page.screenshot({ path: join(artifacts, 'content-search.png') });
    assert.deepEqual(await labels(), ['Needle index', 'Content one', 'Content two']);

    await field.fill('ActualLeaf');
    await row('Independent').waitFor({ state: 'attached' });
    assert.equal(await row('Independent').getAttribute('aria-description'), 'ActualLeaf');
    await field.fill('Secret-ancestor');
    await requested('Secret-ancestor');
    await finish('Secret-ancestor');
    await settled('无匹配会话');
    assert.equal(await rows.count(), 0, 'ancestors of a cwd are outside the metadata search keys');

    await field.fill('41abc');
    await row('Review workflow').waitFor({ state: 'attached' });
    await requested('41abc');
    await field.press('Escape');
    await focus('搜索会话', 'BUTTON');
    assert.equal(await trigger.getAttribute('aria-expanded'), 'false');
    await page.getByRole('tree', { name: '工作区和会话' }).waitFor({ state: 'attached' });
    const generation = (await searchCalls()).find(call => call.query === '41abc').generation;
    await page.waitForFunction(generation => window.__verification.calls.some(call =>
      call.name === 'search_session_history' && call.args.query == null && call.args.generation > generation),
    generation, { timeout: 5000 });
    await finish('41abc', [{ sessionId: 'search-41abc', snippet: 'Late cancelled hit' }]);

    await trigger.press('Enter');
    await focus('搜索会话名称', 'INPUT');
    assert.equal(await field.inputValue(), '');
    await field.fill('41abc');
    await row('Review workflow').getByRole('button', { name: '打开 Review workflow' }).dispatchEvent('click');
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'resume_session'
      && call.args.sessionId === 'search-41abc'), null, { timeout: 5000 });
    await page.getByRole('tree', { name: '工作区和会话' }).getByRole('treeitem', { name: 'Review workflow', exact: true })
      .waitFor({ state: 'attached' });
    assert.equal(await trigger.getAttribute('aria-expanded'), 'false');

    await page.waitForTimeout(250); // The collapsed view-options button slides back into place.
    await page.mouse.click(224, 283);
    await page.waitForTimeout(250); // The options menu animates in before its rows can be clicked.
    await page.screenshot({ path: join(artifacts, 'archive-filter.png') });
    await page.mouse.click(122, 577);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'save_workspace_preferences'
      && call.args.preferences.archivedFilter === 'show'), null, { timeout: 5000 });
    await trigger.dispatchEvent('click');
    await focus('搜索会话名称', 'INPUT');
    await field.fill('needle');
    const archived = row('Needle of archive');
    const restore = archived.getByRole('button', { name: '取消归档', exact: true });
    await restore.waitFor({ state: 'attached' });
    await archived.getByRole('button', { name: '打开 Needle of archive', exact: true }).dispatchEvent('click');
    assert.equal(await field.inputValue(), 'needle');
    assert.equal(await page.evaluate(() => window.__verification.calls
      .filter(call => call.name === 'resume_session' && call.args.sessionId === 'archived-match').length), 0);
    await restore.press('Enter');
    await restore.waitFor({ state: 'detached' });
    assert.equal(await field.inputValue(), 'needle');
    assert.deepEqual(await page.evaluate(() => window.__verification.calls
      .filter(call => call.name === 'set_session_archived').map(call => call.args)), [
      { sessionId: 'archived-match', archived: false, stopActivity: false },
    ]);
    await field.press('Escape');
    await focus('搜索会话', 'BUTTON');
    const expanded = value => page.waitForFunction(value => document.querySelector(
      '#workspace-accessibility-search button')?.getAttribute('aria-expanded') === value, String(value), { timeout: 5000 });
    // Operate the actual GPUI canvas as well as the webview accessibility controls.
    await page.locator('canvas').focus();
    await page.keyboard.press('ControlOrMeta+k');
    await expanded(true);
    await page.keyboard.insertText('needle');
    await row('Needle index').waitFor({ state: 'attached' });
    await page.mouse.click(530, 90);
    await page.keyboard.insertText(' outside');
    assert.equal(await field.inputValue(), 'needle');
    await page.waitForTimeout(200); // Clear moves to the field's right edge as it expands.
    await page.mouse.click(251, 284);
    await expanded(false);
    await page.keyboard.press('Enter');
    await expanded(true);
    assert.equal(await field.inputValue(), '');
    await page.mouse.click(530, 90);
    await expanded(false);
  } else if (mode === 'workspace-accessibility') {
    const tree = page.getByRole('tree', { name: '工作区和会话' });
    const parent = tree.getByRole('treeitem', { name: 'Host parent', exact: true });
    const child = parent.getByRole('treeitem', { name: 'Host child', exact: true });
    const ungrouped = tree.getByRole('treeitem', { name: '未分组', exact: true });
    const outsider = ungrouped.getByRole('treeitem', { name: 'Ungrouped despite same cwd', exact: true });
    const state = (attribute, value) => page.waitForFunction(({ attribute, value }) =>
      document.activeElement?.getAttribute(attribute) === value, { attribute, value }, { timeout: 5000 });
    await outsider.waitFor({ state: 'attached' });
    assert.equal(await child.getAttribute('aria-level'), '2');
    assert.equal(await outsider.getAttribute('aria-selected'), 'false');
    await parent.focus();
    await page.keyboard.press('ArrowRight');
    await state('aria-label', 'Host child');
    await page.keyboard.press('ArrowLeft');
    await state('aria-expanded', 'false');
    await page.keyboard.press('ArrowLeft');
    await state('aria-label', 'Host parent');
    await page.keyboard.press('Space');
    await child.waitFor({ state: 'detached' });
    assert.equal(await parent.getAttribute('aria-expanded'), 'false');
    await page.keyboard.press('ArrowRight');
    await child.waitFor({ state: 'attached' });
    await page.keyboard.press('ArrowRight');
    await state('aria-label', 'Host child');
    await parent.getByRole('button', { name: '折叠 Host parent', exact: true }).dispatchEvent('click');
    await child.waitFor({ state: 'detached' });
    await state('aria-label', 'Host parent');
    await page.keyboard.press('End');
    await state('aria-label', 'Ungrouped despite same cwd');
    const sessionMenu = tree => tree.getByRole('button', { name: /会话“.*”的操作/u });
    const menu = page.getByRole('menu', { name: /会话“.*”的操作/u });
    const menuItem = name => menu.getByRole('menuitem', { name, exact: true });
    const focusedText = label => page.waitForFunction(label => document.activeElement?.textContent === label, label, { timeout: 5000 });
    const calls = name => page.evaluate(name => window.__verification.calls.filter(call => call.name === name).map(call => call.args), name);
    const waitCall = name => page.waitForFunction(name => window.__verification.calls.some(call => call.name === name), name, { timeout: 5000 });
    await page.keyboard.press('Shift+F10');
    await menu.waitFor({ state: 'attached' });
    await focusedText('置顶会话');
    await page.waitForTimeout(250);
    const menuClip = { x: 12, y: 306, width: 270, height: 414 };
    const rightEdge = { x: menuClip.x + 133, y: menuClip.y + 156, width: 2, height: 12 };
    const pinEdge = await page.screenshot({ clip: rightEdge });
    const pinFocus = await page.screenshot({ path: join(artifacts, 'pin-focus.png'), clip: menuClip });
    await page.keyboard.press('ArrowDown');
    await focusedText('重命名');
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    const renameFocus = await page.screenshot({ path: join(artifacts, 'rename-focus.png'), clip: menuClip });
    assert.ok(!pinFocus.equals(renameFocus), 'moving menu focus also updates the painted menu');
    assert.ok(!pinEdge.equals(await page.screenshot({ clip: rightEdge })),
      'the first row highlight reaches the right edge beyond its short label');
    await page.keyboard.press('ArrowDown');
    await focusedText('分叉会话');
    assert.equal(await menuItem('标为未读').isDisabled(), true);
    await menuItem('在应用中打开').focus();
    await page.keyboard.press('ArrowRight');
    await focusedText('Fixture editor');
    await page.keyboard.press('ArrowLeft');
    await focusedText('在应用中打开');
    await page.keyboard.press('Escape');
    await menu.waitFor({ state: 'detached' });
    await focusedText('会话“Ungrouped despite same cwd”的操作');

    await sessionMenu(outsider).dispatchEvent('click');
    await menuItem('置顶会话').dispatchEvent('click');
    await page.waitForFunction(() => Boolean(window.__verification.settleWorkspacePin), null, { timeout: 5000 });
    assert.deepEqual((await calls('workspace_registry')).at(-1).operation, { action: 'pin', sessionId: outsiderId });
    await page.evaluate(snapshot => window.__verification.settleWorkspacePin(snapshot), { available: true, sequence: 2, items: hostWorkspaces, pinned: [outsiderId], archived: [] });
    await sessionMenu(outsider).dispatchEvent('click');
    await menuItem('取消置顶').waitFor({ state: 'attached' });
    await menuItem('重命名').dispatchEvent('click');
    const rename = page.getByRole('dialog', { name: '重命名会话' });
    const name = rename.getByRole('textbox', { name: '会话名称' });
    await name.waitFor({ state: 'attached' });
    assert.equal(await name.inputValue(), 'Ungrouped despite same cwd');
    await name.fill('');
    const saveRename = rename.getByRole('button', { name: '保存', exact: true });
    await saveRename.and(rename.locator(':disabled')).waitFor({ state: 'attached', timeout: 5000 });
    await name.fill('Renamed through accessibility');
    await name.press('Enter');
    await waitCall('rename_session');
    assert.deepEqual(await calls('rename_session'), [{ sessionId: outsiderId, title: 'Renamed through accessibility' }]);
    await page.evaluate(() => window.__verification.settleSessionRename({ error: '重命名失败' }));
    await rename.getByRole('alert').filter({ hasText: '重命名失败' }).waitFor({ state: 'attached' });
    assert.equal(await name.inputValue(), 'Renamed through accessibility');
    await name.press('Enter');
    await page.waitForFunction(() => window.__verification.calls.filter(call => call.name === 'rename_session').length === 2, null, { timeout: 5000 });
    await page.evaluate(() => window.__verification.settleSessionRename({ title: 'Renamed through accessibility', seq: 20 }));
    const renamed = ungrouped.getByRole('treeitem', { name: 'Renamed through accessibility', exact: true });
    await renamed.waitFor({ state: 'attached' });
    await focusedText('会话“Renamed through accessibility”的操作');
    await renamed.focus();
    await page.keyboard.press('Enter');
    await page.waitForFunction(id => window.__verification.calls.some(call =>
      call.name === 'resume_session' && call.args.sessionId === id), outsiderId, { timeout: 5000 });
    await renamed.and(tree.getByRole('treeitem', { selected: true })).waitFor({ state: 'attached' });
    await sessionMenu(renamed).dispatchEvent('click');
    await menuItem('归档会话').dispatchEvent('click');
    const archive = page.getByRole('alertdialog', { name: '停止并归档此会话？' });
    await archive.getByText('进行中的回合').waitFor({ state: 'attached' });
    await archive.getByRole('button', { name: '取消', exact: true }).dispatchEvent('click');
    await renamed.waitFor({ state: 'attached' });
    await sessionMenu(renamed).dispatchEvent('click');
    await menuItem('归档会话').dispatchEvent('click');
    await archive.getByRole('button', { name: '停止并归档', exact: true }).dispatchEvent('click');
    await renamed.waitFor({ state: 'detached' });
    assert.deepEqual((await calls('set_session_archived')).map(({ sessionId, stopActivity }) => ({ sessionId, stopActivity })), [
      { sessionId: outsiderId, stopActivity: false }, { sessionId: outsiderId, stopActivity: false }, { sessionId: outsiderId, stopActivity: true },
    ]);
  } else if (mode === 'workspace-registry') {
    const registryCalls = () => page.evaluate(() => window.__verification.calls.filter(call => call.name === 'workspace_registry').map(call => call.args.operation));
    const movePending = () => page.waitForFunction(() => Boolean(window.__verification.settleWorkspaceMove), null, { timeout: 5000 });
    const settleMove = (result) => page.evaluate(result => {
      window.__verification.settleWorkspaceMove(result);
      delete window.__verification.settleWorkspaceMove;
    }, result);
    const snapshot = (sequence, items, pinned = [], archived = []) => ({ available: true, sequence, items, pinned, archived });
    const sidebar = name => page.screenshot({ path: join(artifacts, `${name}.png`), clip: { x: 12, y: 306, width: 256, height: 180 } });
    const dragGroup = async (from, to, cancel = false) => {
      await page.mouse.move(78, from);
      await page.mouse.down();
      await page.mouse.move(78, to, { steps: 8 });
      await page.waitForTimeout(150);
      if (cancel) await page.keyboard.press('Escape');
      await page.mouse.up();
      await page.mouse.move(400, 284);
    };
    await page.waitForTimeout(650);
    const initial = await sidebar('host-order');
    await dragGroup(322, 404, true);
    await page.waitForTimeout(250);
    assert.deepEqual(await registryCalls(), [], 'Escape cancels a Workspace drag');
    await dragGroup(322, 404);
    await movePending();
    const [attempt] = await registryCalls();
    assert.deepEqual({ ...attempt, beforeId: attempt.beforeId ?? null }, { action: 'move', id: 'host-sibling', beforeId: null });
    assert.ok((await sidebar('while-host-saving')).equals(initial), 'the list waits for Host confirmation');
    await settleMove({ error: 'fixture registry write rejected' });
    await page.waitForTimeout(300);
    assert.ok((await sidebar('host-rejected')).equals(initial), 'a rejected write leaves the old order visible');

    const [sibling, parent, child] = hostWorkspaces;
    const reordered = [parent, child, sibling];
    await dragGroup(322, 404);
    await movePending();
    await settleMove(snapshot(2, reordered));
    await page.waitForTimeout(300);
    assert.ok(!(await sidebar('host-committed')).equals(initial), 'the committed Host order moves the subtree as a block');
    await dragGroup(432, 311);
    await movePending();
    assert.deepEqual((await registryCalls()).at(-1), { action: 'move', id: 'host-sibling', beforeId: 'host-parent' });
    await page.evaluate(value => window.__verification.emit({ kind: 'workspace_registry', ...value }), snapshot(4, hostWorkspaces));
    await page.waitForTimeout(250);
    assert.ok((await sidebar('host-notification')).equals(initial), 'a later Host notification updates the list');
    await settleMove(snapshot(3, reordered));
    await page.waitForTimeout(250);
    assert.ok((await sidebar('late-host-reply')).equals(initial), 'an old write response cannot overwrite the newer Host state');

    const pinFromRow = async (y, action) => {
      await page.mouse.click(82, y, { button: 'right' });
      await page.waitForTimeout(240);
      const menu = await page.screenshot();
      await page.mouse.click(126, y + 24);
      await page.waitForFunction(() => Boolean(window.__verification.settleWorkspacePin), null, { timeout: 5000 }).catch(async error => {
        await writeFile(join(artifacts, `pin-menu-${action}-failed.png`), menu);
        console.error('WORKSPACE_PIN_ACTION', action, JSON.stringify(await registryCalls()));
        throw error;
      });
      assert.deepEqual((await registryCalls()).at(-1), { action, sessionId: outsiderId });
    };
    const settlePin = value => page.evaluate(value => {
      window.__verification.settleWorkspacePin(value);
      delete window.__verification.settleWorkspacePin;
    }, value);
    await page.screenshot({ path: join(artifacts, 'ungrouped-before-pin.png') });
    await pinFromRow(496, 'pin');
    await settlePin({ error: 'fixture pin rejected' });
    await page.waitForTimeout(280);
    await pinFromRow(496, 'pin');
    await settlePin(snapshot(5, hostWorkspaces, [outsiderId]));
    await page.waitForTimeout(280);
    await pinFromRow(496, 'unpin');
    await page.evaluate(value => window.__verification.emit({ kind: 'workspace_registry', ...value }), snapshot(7, hostWorkspaces, [], [outsiderId]));
    await page.waitForTimeout(280);
    const captureMemberArea = () => page.screenshot({ clip: { x: 12, y: 411, width: 256, height: 117 } });
    const archivedArea = await captureMemberArea();
    await settlePin(snapshot(6, hostWorkspaces));
    await page.waitForTimeout(250);
    assert.ok((await captureMemberArea()).equals(archivedArea), 'a stale pin reply leaves the newer archive in effect');

    const attached = [sibling, { ...parent, sessionIds: [outsiderId, sessionId] }, child];
    await page.evaluate(value => window.__verification.emit({ kind: 'workspace_registry', ...value }), snapshot(8, attached));
    await page.waitForTimeout(280);
    await page.screenshot({ path: join(artifacts, 'host-member-restored.png') });
    await page.mouse.click(82, 431);
    await page.waitForFunction(id => window.__verification.calls.some(call => call.name === 'resume_session' && call.args.sessionId === id), outsiderId, { timeout: 5000 });

    await page.mouse.click(258, 283);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'new_session'), null, { timeout: 5000 });
    assert.deepEqual((await registryCalls()).at(-1), { action: 'create', path: chosenDirectory });
    assert.deepEqual(await page.evaluate(() => window.__verification.calls.find(call => call.name === 'new_session').args), { cwd: canonicalDirectory, workspaceId: 'host-picked' });
  } else if (mode === 'workspace-group-accessibility') {
    const [sibling, parent, child] = hostWorkspaces;
    const tree = page.getByRole('tree', { name: '工作区和会话' });
    const group = title => tree.getByRole('treeitem', { name: title, exact: true });
    const trigger = title => group(title).getByRole('button', { name: `工作区“${title}”的操作`, exact: true });
    const menu = page.getByRole('menu', { name: /工作区“.*”的操作/u });
    const item = name => menu.getByRole('menuitem', { name, exact: true });
    const rename = page.getByRole('dialog', { name: '重命名工作区', exact: true });
    const renameField = rename.getByRole('textbox', { name: '工作区名称', exact: true });
    const remove = page.getByRole('dialog', { name: '移除工作区', exact: true });
    const focused = label => page.waitForFunction(label => document.activeElement?.textContent === label, label, { timeout: 5000 });
    const operations = () => page.evaluate(() => window.__verification.calls.filter(call => call.name === 'workspace_registry').map(call => call.args.operation));
    const pending = async expected => {
      await page.waitForFunction(() => Boolean(window.__verification.settleWorkspaceAction), null, { timeout: 5000 });
      assert.deepEqual((await operations()).at(-1), expected);
    };
    const settle = value => page.evaluate(value => {
      window.__verification.settleWorkspaceAction(value);
      delete window.__verification.settleWorkspaceAction;
    }, value);
    const snapshot = (sequence, items) => ({ available: true, sequence, items, pinned: [], archived: [] });
    const open = async (title, action) => {
      await trigger(title).dispatchEvent('click');
      await item(action).dispatchEvent('click');
    };

    await group(parent.title).focus();
    await page.keyboard.press('Shift+F10');
    await focused('重命名');
    await page.keyboard.press('End');
    await focused('移除工作区');
    await page.keyboard.press('ArrowDown');
    await focused('重命名');
    await page.keyboard.press('Escape');
    await focused(`工作区“${parent.title}”的操作`);
    await trigger(parent.title).press('Enter');
    await item(/在.*中显示/u).dispatchEvent('click');
    await page.waitForFunction(path => window.__verification.calls.some(call => call.name === 'open_start_tool'
      && call.args.workspace === path && call.args.tool === 'files'), cwd, { timeout: 5000 });

    await open(parent.title, '重命名');
    await renameField.waitFor({ state: 'attached' });
    assert.equal(await renameField.inputValue(), parent.title);
    assert.equal(await rename.getByRole('button', { name: '重命名', exact: true }).isDisabled(), true);
    await renameField.fill(child.title);
    await rename.getByRole('alert').filter({ hasText: child.title }).waitFor({ state: 'attached' });
    await renameField.press('Enter');
    assert.deepEqual(await operations(), []);
    const title = 'Renamed through the workspace menu';
    await renameField.fill(`  ${title}  `);
    await renameField.press('Enter');
    await pending({ action: 'rename', id: parent.id, title });
    await renameField.press('Escape');
    assert.equal(await rename.getByRole('button', { name: '取消', exact: true }).isDisabled(), true);
    await settle({ error: '工作区改名失败' });
    await rename.getByRole('alert').filter({ hasText: '工作区改名失败' }).waitFor({ state: 'attached' });
    assert.equal(await renameField.inputValue(), `  ${title}  `);
    await renameField.press('Enter');
    await pending({ action: 'rename', id: parent.id, title });
    await settle(snapshot(2, [sibling, { ...parent, title }, child]));
    await focused(`工作区“${title}”的操作`);

    await open(title, '移除工作区');
    await remove.getByText(/文件夹与会话记录会保留/u).waitFor({ state: 'attached' });
    await remove.getByRole('button', { name: '取消' }).dispatchEvent('click');
    await focused(`工作区“${title}”的操作`);

    await open(title, '重命名');
    await renameField.fill('Stale draft');
    const replacement = { ...parent, id: 'replacement-workspace', title: 'Replacement at the same path' };
    await page.evaluate(value => window.__verification.emit({ kind: 'workspace_registry', ...value }), snapshot(3, [sibling, replacement, child]));
    await rename.getByRole('alert').filter({ hasText: '工作区已移除或替换' }).waitFor({ state: 'attached' });
    assert.equal(await rename.getByRole('button', { name: '重命名', exact: true }).isDisabled(), true);
    await renameField.press('Enter');
    assert.equal((await operations()).length, 2);
    await renameField.press('Escape');
    await rename.waitFor({ state: 'detached' });

    await open(replacement.title, '移除工作区');
    await remove.getByRole('button', { name: '移除工作区', exact: true }).dispatchEvent('click');
    await pending({ action: 'delete', id: replacement.id });
    await remove.getByRole('status').filter({ hasText: '正在移除工作区' }).waitFor({ state: 'attached' });
    await settle({ error: '工作区移除失败' });
    await remove.getByRole('alert').filter({ hasText: '工作区移除失败' }).waitFor({ state: 'attached' });
    await remove.getByRole('button', { name: '移除工作区', exact: true }).dispatchEvent('click');
    await pending({ action: 'delete', id: replacement.id });
    await settle(snapshot(4, [sibling, child]));
    await remove.waitFor({ state: 'detached' });
    assert.equal(await group(replacement.title).count(), 0);
    const ungrouped = group('未分组');
    const retained = ungrouped.getByRole('treeitem', { name: 'Ungrouped despite same cwd', exact: true });
    await retained.waitFor({ state: 'attached' });
    assert.equal(await ungrouped.getByRole('button', { name: '工作区“未分组”的操作', exact: true }).count(), 0);
    await retained.focus();
    await page.keyboard.press('Enter');
    await page.waitForFunction(id => window.__verification.calls.some(call => call.name === 'resume_session'
      && call.args.sessionId === id), outsiderId, { timeout: 5000 });
  } else if (mode === 'workspace-actions') {
    const [sibling, parent, child] = hostWorkspaces;
    const calls = () => page.evaluate(() => window.__verification.calls.filter(call => call.name === 'workspace_registry').map(call => call.args.operation));
    const shot = name => page.screenshot({ path: join(artifacts, `${name}.png`) });
    const sidebar = () => page.screenshot({ clip: { x: 12, y: 306, width: 256, height: 212 } });
    const snapshot = (sequence, items) => ({ available: true, sequence, items, pinned: [], archived: [] });
    const emit = value => page.evaluate(value => window.__verification.emit({ kind: 'workspace_registry', ...value }), value);
    const pending = async expected => {
      await page.waitForFunction(() => Boolean(window.__verification.settleWorkspaceAction), null, { timeout: 5000 }).catch(async error => {
        await shot(`${expected.action}-request-failed`);
        console.error('WORKSPACE_ACTION_CALLS', JSON.stringify(await calls()));
        throw error;
      });
      assert.deepEqual((await calls()).at(-1), expected);
    };
    const settle = value => page.evaluate(value => {
      window.__verification.settleWorkspaceAction(value);
      delete window.__verification.settleWorkspaceAction;
    }, value);
    const open = async (action, keyboard = false) => {
      await page.mouse.click(78, 360, { button: 'right' });
      await page.waitForTimeout(230);
      if (keyboard) {
        await page.keyboard.press('End');
        await page.keyboard.press('ArrowDown');
        await page.keyboard.press('Enter');
      } else {
        await page.mouse.click(350, action === 'rename' ? 368 : 439);
      }
      await page.waitForTimeout(250);
    };
    await page.waitForTimeout(550);
    await shot('host-label');
    await open('rename', true);
    await page.keyboard.type('Host child');
    await page.keyboard.press('Enter');
    await page.waitForTimeout(200);
    await shot('rename-duplicate');
    assert.deepEqual(await calls(), [], 'a duplicate title in another registered workspace cannot be submitted');
    await page.keyboard.press('ControlOrMeta+A');
    await page.keyboard.type('  New Host title  ');
    const rename = { action: 'rename', id: parent.id, title: 'New Host title' };
    await page.keyboard.press('Enter');
    await pending(rename);
    await page.keyboard.press('Escape');
    await page.keyboard.press('Enter');
    assert.deepEqual(await calls(), [rename], 'the pending rename accepts just one submission');
    await settle({ error: 'fixture rename failed' });
    await page.waitForTimeout(250);
    await shot('rename-failed');
    await page.keyboard.press('Enter');
    await pending(rename);
    const newer = [sibling, { ...parent, title: 'Latest Host title' }, child];
    await emit(snapshot(3, newer));
    await settle(snapshot(2, [sibling, { ...parent, title: rename.title }, child]));
    await page.waitForTimeout(300);
    const renamed = await sidebar();
    await shot('rename-committed');
    await emit(snapshot(4, newer));
    await page.waitForTimeout(250);
    assert.ok((await sidebar()).equals(renamed), 'completing a rename preserves the newer Host title');

    await open('delete');
    await page.keyboard.press('Escape');
    assert.deepEqual(await calls(), [rename, rename]);
    await open('delete');
    const remove = { action: 'delete', id: parent.id };
    await page.keyboard.press('Tab');
    await page.keyboard.press('Enter');
    await pending(remove);
    await shot('remove-pending');
    await page.keyboard.press('Escape');
    await settle({ error: 'fixture removal failed' });
    await page.waitForTimeout(250);
    await shot('remove-failed');
    await page.keyboard.press('Enter');
    await pending(remove);
    await settle(snapshot(5, [sibling, child]));
    await page.waitForTimeout(300);
    await shot('remove-committed');
    assert.ok(!(await sidebar()).equals(renamed), 'the committed removal updates the sidebar');
    await page.mouse.click(82, 431);
    await page.waitForFunction(id => window.__verification.calls.some(call => call.name === 'resume_session' && call.args.sessionId === id), outsiderId, { timeout: 5000 });
  } else if (mode === 'session-order') {
    const flat = '__flat_session_order__';
    const originalFlat = [sessionId, 'pin-alpha', 'first', 'archived', 'other-first', 'second', 'pin-beta', 'other-second'];
    const grouped = [sessionId, 'second', 'pin-alpha', 'first', 'archived', 'pin-beta'];
    const reorderedFlat = [sessionId, 'pin-alpha', 'first', 'archived', 'other-second', 'other-first', 'second', 'pin-beta'];
    const saved = () => page.evaluate(() => JSON.parse(sessionStorage.getItem('kwork-test-session-order')));
    const saves = () => page.evaluate(() => window.__verification.calls.filter(call => call.name === 'save_workspace_preferences').length);
    const waitForSave = count => page.waitForFunction(count => window.__verification.calls.filter(call => call.name === 'save_workspace_preferences').length > count, count, { timeout: 5000 });
    const drag = async (from, to, { cancel, outside } = {}) => {
      await page.mouse.move(95, from);
      await page.mouse.down();
      await page.mouse.move(95, to, { steps: 8 });
      await page.waitForTimeout(150);
      if (cancel) await page.keyboard.press('Escape');
      if (outside) await page.mouse.move(350, to, { steps: 8 });
      await page.mouse.up();
    };
    const selectViewOption = async y => {
      const count = await saves();
      await page.mouse.click(224, 283);
      await page.waitForTimeout(250);
      await page.mouse.click(122, y);
      await waitForSave(count);
      await page.mouse.move(400, 284);
      await page.waitForTimeout(250);
    };
    const sidebar = name => page.screenshot({ path: join(artifacts, `${name}.png`), clip: { x: 12, y: 306, width: 256, height: 318 } });
    await page.waitForTimeout(700);
    const initialSaves = await saves();
    await drag(492, 448, { cancel: true });
    await drag(492, 390);
    await page.waitForTimeout(250);
    assert.equal(await saves(), initialSaves, 'Escape and dropping on a pinned row leave ordinary order unchanged');
    await drag(492, 345, { outside: true });
    await waitForSave(initialSaves);
    assert.deepEqual((await saved()).sessionOrderByAccount, {
      [flat]: originalFlat, [cwd]: grouped, [orderWorkspace]: ['other-first', 'other-second'],
    }, 'the first drag freezes every account, moves after the scratch row and retains the archive slot');
    assert.equal((await saved()).orderBy, 'manual');

    await selectViewOption(402);
    assert.equal((await saved()).groupBy, 'flat');
    const beforeFlat = await saves();
    await drag(526, 448);
    await waitForSave(beforeFlat);
    assert.deepEqual((await saved()).sessionOrderByAccount[flat], reorderedFlat);
    assert.deepEqual((await saved()).sessionOrderByAccount[cwd], grouped);
    await selectViewOption(372);
    assert.equal((await saved()).groupBy, 'workspaceTree');
    await selectViewOption(402);
    assert.deepEqual((await saved()).sessionOrderByAccount[flat], reorderedFlat, 'switching from tree restores the flat order');
    const beforeReload = await sidebar('flat-before-reload');

    await page.reload();
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'turn_token_usage'), null, { timeout: 10000 });
    await page.waitForTimeout(500);
    assert.ok((await sidebar('flat-after-reload')).equals(beforeReload), 'reloading restores the rendered flat order');
    await selectViewOption(577);
    assert.deepEqual((await saved()).sessionOrderByAccount[flat], reorderedFlat, 'revealing archives does not change saved positions');
    await selectViewOption(490);
    assert.equal((await saved()).orderBy, 'updated');
    assert.deepEqual((await saved()).sessionOrderByAccount, {});
    await selectViewOption(459);
    assert.deepEqual((await saved()).sessionOrderByAccount[flat], originalFlat, 'entering manual again freezes the order from persisted prompt times');

    const beforeFirstPrompt = await saves();
    await page.mouse.click(618, 984);
    await page.waitForFunction(() => document.activeElement instanceof HTMLTextAreaElement && !document.activeElement.readOnly, null, { timeout: 5000 });
    await page.keyboard.type('First message in the scratch session');
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => Boolean(window.__verification.completePrompt), null, { timeout: 5000 });
    await waitForSave(beforeFirstPrompt);
    const afterFirstPrompt = await saved();
    assert.equal(afterFirstPrompt.scratchSession ?? null, null);
    assert.deepEqual(afterFirstPrompt.sessionOrderByAccount[flat], originalFlat);
    assert.equal(afterFirstPrompt.sessionOrderByAccount[cwd][0], sessionId, 'the first prompt preserves the provisional manual slot');
    await page.evaluate(() => window.__verification.completePrompt());
  } else if (mode === 'goal') {
    const draft = 'unsent chat draft';
    const objective = 'first line\nsecond line';
    const waitForInput = async (text) => {
      // The hidden IME textarea syncs after an input event; focus alone can leave its old value in place.
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      await page.keyboard.press('End');
      await page.waitForFunction(value => document.activeElement?.value === value, text, { timeout: 5000 }).catch(async error => {
        const actual = await page.evaluate(() => ({ tag: document.activeElement?.tagName, value: document.activeElement?.value }));
        await page.screenshot({ path: join(artifacts, 'failed-input.png') });
        throw new Error(`Expected input ${JSON.stringify(text)}, got ${JSON.stringify(actual)}`, { cause: error });
      });
    };
    const actionCalls = () => page.evaluate(() => window.__verification.calls.filter(call => call.name === 'goal_action'));
    const openEdit = () => page.mouse.click(1154, 934);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'goal_state'), null, { timeout: 5000 });
    await page.waitForTimeout(300);
    await page.mouse.click(601, 984);
    await page.waitForFunction(() => document.activeElement instanceof HTMLTextAreaElement && !document.activeElement.readOnly, null, { timeout: 5000 });
    await page.keyboard.type(draft);
    await waitForInput(draft);
    await openEdit();
    await waitForInput('原始目标');
    await page.keyboard.press('Escape');
    await waitForInput(draft);
    assert.deepEqual(await actionCalls(), []);

    await openEdit();
    await waitForInput('原始目标');
    await page.keyboard.press('ControlOrMeta+A');
    await page.keyboard.type('first line');
    await page.keyboard.press('Shift+Enter');
    await page.keyboard.type('second line');
    await waitForInput(objective);
    await page.screenshot({ path: join(artifacts, 'multiline-edit.png') });
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => Boolean(window.__verification.settleGoal), null, { timeout: 5000 });
    await page.keyboard.press('Enter');
    assert.deepEqual((await actionCalls()).map(({ args }) => args), [
      { sessionId, action: 'edit', goalId: 'fixture-goal', revision: 1, objective },
    ]);

    await page.evaluate(() => {
      window.__verification.settleGoal('保存失败，请重试');
      delete window.__verification.settleGoal;
    });
    await waitForInput(objective);
    await page.screenshot({ path: join(artifacts, 'retry.png') });
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => Boolean(window.__verification.settleGoal), null, { timeout: 5000 });
    await page.evaluate(objective => {
      const goal = { id: 'fixture-goal', revision: 2, objective, phase: 'paused', activation: 'disarmed' };
      window.__verification.settleGoal(null, { goal, sequence: 3 });
    }, objective);
    await waitForInput(draft);
    await page.evaluate(sessionId => {
      window.__verification.emit({ kind: 'session_update', sessionId, update: {
        sessionUpdate: 'gpui_goal_changed', sequence: 2,
        goal: { id: 'fixture-goal', revision: 1, objective: '过时的状态', phase: 'paused', activation: 'disarmed' },
      } });
    }, sessionId);
    await openEdit();
    await waitForInput(objective);
    assert.equal((await actionCalls()).length, 2);
    assert.equal(await page.evaluate(() => window.__verification.calls.some(call => call.name === 'prompt')), false);
    await page.keyboard.press('Escape');
    await waitForInput(draft);
  } else if (mode === 'reading') {
    const focus = (active) => page.evaluate(active => {
      document.hasFocus = () => active;
      window.dispatchEvent(new Event(active ? 'focus' : 'blur'));
    }, active);
    const showFailure = () => page.evaluate(sessionId => {
      const emit = window.__verification.emit;
      emit({ kind: 'session_update', sessionId, update: {
        sessionUpdate: 'turn_trigger', messageId: 'scheduled-fixture', sourceKind: 'schedule', content: '定时运行',
      } });
      emit({ kind: 'session_update', sessionId, update: {
        sessionUpdate: 'turn_end', type: 'turn/end', seq: 12, turn: 1, reason: 'error',
      } });
      emit({ kind: 'session_read', sessionId, completedTurn: 1, failed: true, readTurn: 0, manualUnread: false });
    }, sessionId);
    const failedDot = async name => {
      const shot = await page.screenshot({ path: join(artifacts, `${name}.png`) });
      return page.evaluate(async png => {
        const img = new Image();
        img.src = `data:image/png;base64,${png}`;
        await img.decode();
        const canvas = document.createElement('canvas');
        canvas.width = img.width;
        canvas.height = img.height;
        const context = canvas.getContext('2d');
        context.drawImage(img, 0, 0);
        const { data } = context.getImageData(15, 340, 30, 32);
        for (let i = 0; i < data.length; i += 4) {
          if (data[i] > 180 && data[i] - data[i + 1] > 70 && data[i] - data[i + 2] > 70) return true;
        }
        return false;
      }, shot.toString('base64'));
    };
    await page.waitForTimeout(300);
    await focus(false);
    await showFailure();
    await page.waitForTimeout(250);
    assert.equal(await failedDot('background'), true, 'an open background conversation shows its failure dot');
    assert.equal(await page.evaluate(() => window.__verification.calls.some(call => call.name === 'session_read_state')), false);
    await focus(true);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'session_read_state' && call.args.action.type === 'acknowledge' && call.args.action.throughTurn === 1), null, { timeout: 5000 });
    await page.waitForTimeout(250);
    assert.equal(await failedDot('foreground'), false, 'foreground acknowledgement clears the dot');
  } else if (mode === 'workflow') {
    await page.waitForTimeout(600);
    await page.screenshot({ path: join(artifacts, 'running.png') });
    await page.mouse.click(900, 167);
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(artifacts, 'phases.png') });
    await page.mouse.click(900, 225);
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(artifacts, 'members.png') });
    await page.evaluate(() => {
      const emit = (event, running) => window.__verification.emit({ kind: 'session_update', sessionId: 'session-markdown-tools-browser', update: running === undefined ? { sessionUpdate: 'gpui_ptc_dispatch', event } : { sessionUpdate: 'gpui_agent_status', sessionId: 'child-api', parentSessionId: 'session-markdown-tools-browser', running } });
      emit({ type: 'tool-workflow/agent-start', data: { runId: 7, seq: 3, label: '验证构建', childId: 'child-build', phase: '验证' } });
      emit({ type: 'tool-workflow/agent-end', data: { runId: 7, seq: 2, outcome: 'failed' } });
      emit(undefined, true);
    });
    await page.waitForTimeout(500);
    await page.screenshot({ path: join(artifacts, 'live-updated.png') });
    await page.mouse.click(900, 273);
    await page.waitForTimeout(700);
    await page.screenshot({ path: join(artifacts, 'child-session.png') });
    await writeFile(join(artifacts, 'calls.json'), JSON.stringify(await page.evaluate(() => window.__verification.calls), null, 2));
  } else if (mode === 'model-menu') {
    const route = (provider, model) => JSON.stringify([provider, model]);
    const calls = () => page.evaluate(() => window.__verification.calls
      .filter(call => call.name === 'set_config_option').map(call => call.args));
    const waitForChoice = async (count, configId, value) => {
      await page.waitForFunction(count => window.__verification.calls
        .filter(call => call.name === 'set_config_option').length === count
        && Boolean(window.__verification.settleModelChoice), count, { timeout: 5000 });
      assert.deepEqual((await calls()).at(-1), { sessionId, configId, value });
    };
    const settle = failure => page.evaluate(failure => window.__verification.settleModelChoice(failure), failure ?? null);
    const publish = scenario => page.evaluate(scenario => window.__verification.updateModelCatalog(scenario), scenario);
    const shot = name => page.screenshot({ path: join(artifacts, `${name}.png`) });
    const open = async () => {
      await page.mouse.click(1140, 1039);
      await page.waitForTimeout(180);
    };
    const drill = async (row = 0) => {
      for (let i = 0; i <= row; i++) await page.keyboard.press('ArrowDown');
      await page.keyboard.press('Enter');
    };
    const replaceQuery = async text => {
      await page.keyboard.press('ControlOrMeta+a');
      await page.keyboard.insertText(text);
    };

    const replay = route('custom-host', 'replay');
    const flash = route('deepseek-official', 'official-matrix-fast');
    await shot('initial');
    await open();
    await shot('root');
    await drill();
    await page.keyboard.insertText('routed telemetry');
    await shot('description-result');
    await page.keyboard.press('Tab');
    await waitForChoice(1, 'model', replay);
    await page.keyboard.press('Tab');
    assert.equal((await calls()).length, 1, 'pending selection accepts one write');
    await settle('fixture selection rejected');
    await page.waitForTimeout(200);
    await shot('retry');
    await page.keyboard.press('Enter');
    await waitForChoice(2, 'model', replay);
    await settle();
    await page.waitForTimeout(200);

    await open();
    await drill();
    await page.keyboard.insertText('matrixfast');
    await shot('model-id-results');
    await page.keyboard.press('Enter');
    await waitForChoice(3, 'model', flash);
    await settle();
    await page.waitForTimeout(200);
    await open();
    await shot('reasoning-root');
    await drill(1);
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Tab');
    await waitForChoice(4, 'reasoning_effort', 'high');
    await settle();
    await page.waitForTimeout(200);
    await open();
    await drill(1);
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('Enter');
    await waitForChoice(5, 'reasoning_effort', '');
    await settle();
    await page.waitForTimeout(200);

    await open();
    await drill();
    await page.keyboard.press('Enter');
    await page.waitForTimeout(150);
    assert.equal((await calls()).length, 5, 'reselecting the current model causes no IPC');
    await open();
    await drill();
    await replaceQuery('nothing');
    await publish('trimmed');
    await page.waitForTimeout(220);
    await shot('catalog-trimmed');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Enter');
    await waitForChoice(6, 'model', replay);
    await settle();
    await page.waitForTimeout(220);

    await publish('unselected');
    await page.waitForTimeout(220);
    await open();
    await page.waitForFunction(() => document.activeElement instanceof HTMLTextAreaElement
      && !document.activeElement.readOnly, null, { timeout: 5000 });
    await page.keyboard.type('native-9a');
    await shot('unselected-model');
    await page.keyboard.press('Enter');
    await waitForChoice(7, 'model', route('deepseek-official', 'native-9a'));
    await settle();
    await page.waitForTimeout(200);

    await page.mouse.click(630, 984);
    await page.keyboard.type('/model');
    await page.keyboard.press('Enter');
    await page.waitForTimeout(200);
    await page.keyboard.type('stable replay');
    await shot('command-model');
    await page.keyboard.press('Enter');
    await waitForChoice(8, 'model', replay);
    await settle();
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    await page.keyboard.press('End');
    await page.waitForFunction(() => document.activeElement instanceof HTMLTextAreaElement
      && document.activeElement.value === '', null, { timeout: 5000 });
  } else if (mode === 'model-accessibility') {
    const root = page.locator('#model-accessibility');
    const trigger = root.locator(':scope > button');
    const search = root.getByRole('searchbox', { name: '搜索模型…' });
    const invoke = locator => locator.dispatchEvent('click');
    const waitReply = key => page.waitForFunction(key => typeof window.__verification[key] === 'function', key, { timeout: 5000 });
    const settle = async (key, value) => {
      await waitReply(key);
      await page.evaluate(({ key, value }) => window.__verification[key](value), { key, value });
    };
    const writes = () => page.evaluate(() => window.__verification.calls.filter(call => call.name === 'set_config_option').map(call => call.args));
    const isFocused = locator => locator.evaluate(node => node === document.activeElement);
    const waitFocus = async selector => {
      try {
        await page.waitForFunction(selector => document.activeElement?.matches(selector), selector, { timeout: 5000 });
      } catch (error) {
        console.error(await root.evaluate(node => ({ focus: document.activeElement?.outerHTML.slice(0, 300), controls: node.outerHTML.slice(0, 1700) })));
        throw error;
      }
    };
    const waitExpanded = expanded => page.waitForFunction(expanded =>
      document.querySelector('#model-accessibility > button')?.getAttribute('aria-expanded') === String(expanded), expanded, { timeout: 5000 });

    await settle('settleCatalog', 'ready');
    await trigger.waitFor();
    assert.equal(await trigger.textContent(), '选择模型，当前 Base Model');
    await invoke(trigger);
    await waitExpanded(true);
    await waitFocus('#model-accessibility > button');
    await page.keyboard.press('ArrowDown');
    await waitFocus('#model-accessibility [role=menuitem]');
    await page.keyboard.press('Enter');
    await search.waitFor();
    await waitFocus('#model-accessibility [role=searchbox]');
    await page.evaluate(() => window.__verification.emit({ kind: 'model_catalog_changed' }));
    await settle('settleCatalog', 'partial');
    await root.getByText('Fixture custom 加载失败：Fixture provider timed out').waitFor();
    await root.getByRole('group', { name: 'Fixture official' }).waitFor();
    assert.equal(await root.getByRole('menuitemradio', { name: 'Stable Replay' }).count(), 0);
    await search.fill('routed telemetry');
    await root.getByText('没有匹配的模型。').waitFor();
    await search.press('Tab');
    const retry = root.getByRole('button', { name: '重试加载模型' });
    assert.ok(await isFocused(retry));
    await retry.press('Enter');
    await root.getByText('正在刷新模型列表…').waitFor();
    assert.equal(await trigger.getAttribute('aria-busy'), 'true');
    await settle('settleCatalog', 'ready');
    const replay = root.getByRole('menuitemradio', { name: 'Stable Replay', checked: false });
    await replay.waitFor();
    assert.equal(await search.inputValue(), 'routed telemetry');
    assert.ok(await isFocused(search));
    assert.equal(await search.getAttribute('aria-activedescendant'), await replay.getAttribute('id'));
    await search.press('Enter');
    await settle('settleModelChoice', 'Fixture write failed');
    await root.getByText('模型操作失败：Fixture write failed').waitFor();
    assert.equal(await search.inputValue(), 'routed telemetry');
    assert.ok(await isFocused(search));
    await invoke(replay);
    await settle('settleModelChoice', null);
    await waitExpanded(false);
    assert.ok(await isFocused(trigger));
    assert.equal(await trigger.textContent(), '选择模型，当前 Stable Replay');
    assert.deepEqual((await writes()).map(({ value }) => JSON.parse(value)), [
      ['custom-host', 'replay'], ['custom-host', 'replay'],
    ]);

    const launcher = root.getByRole('button', { name: '调用指令', exact: true });
    const suggestions = root.getByRole('listbox', { name: '触发候选建议' });
    const editor = page.locator('#gpui-text-input');
    await invoke(launcher);
    await suggestions.getByRole('option', { name: /^文件 \/file/, selected: true }).waitFor();
    await waitFocus('#gpui-text-input');
    assert.match(await editor.getAttribute('aria-label'), /调用指令/);
    await page.keyboard.press('ArrowDown');
    await suggestions.getByRole('option', { name: /^附件 \/attach/, selected: true }).waitFor();
    await page.keyboard.press('Escape');
    await suggestions.waitFor({ state: 'hidden' });
    assert.ok(await isFocused(editor));
    await page.keyboard.type('/model');
    const modelCommand = suggestions.getByRole('option', { name: /^模型 \/model/, selected: true });
    await modelCommand.waitFor();
    assert.equal(await editor.getAttribute('aria-activedescendant'), await modelCommand.getAttribute('id'));
    await invoke(modelCommand);
    const commandChoices = root.getByRole('listbox', { name: '/model 匹配项' });
    const commandSearch = root.getByRole('textbox', { name: '筛选选项' });
    await commandSearch.waitFor();
    await commandSearch.press('Escape');
    await commandSearch.waitFor({ state: 'hidden' });
    await waitFocus('#gpui-text-input');
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '/model', null, { timeout: 5000 });
    await page.keyboard.press('ControlOrMeta+a');
    await page.keyboard.type('/model');
    await invoke(modelCommand);
    await commandChoices.getByRole('option', { name: 'Stable Replay，当前模型', selected: true }).waitFor();
    assert.ok(await isFocused(commandSearch));
    assert.equal(await editor.getAttribute('aria-hidden'), 'true');
    await commandSearch.fill('Flash Trial');
    const flash = commandChoices.getByRole('option', { name: 'Flash Trial', selected: true });
    await flash.waitFor();
    await commandSearch.press('Tab');
    await settle('settleModelChoice', 'Fixture command selection failed');
    await root.getByText('Fixture command selection failed').waitFor();
    assert.equal(await commandSearch.inputValue(), 'Flash Trial');
    await commandSearch.press('Tab');
    await settle('settleModelChoice', null);
    await trigger.waitFor();
    await waitFocus('#gpui-text-input');
    assert.equal(await editor.getAttribute('aria-hidden'), null);
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '', null, { timeout: 5000 });
    assert.match(await trigger.textContent(), /Flash Trial，推理等级 /);
    await invoke(trigger);
    await invoke(root.getByRole('menuitem', { name: /推理等级，当前/ }));
    await root.getByRole('menuitemradio', { name: 'Default', checked: true }).waitFor();
    await page.keyboard.press('ArrowDown');
    await page.waitForFunction(() => document.activeElement?.textContent === 'High', null, { timeout: 5000 });
    await page.keyboard.press('Tab');
    await settle('settleModelChoice', null);
    await waitExpanded(false);
    assert.equal(await trigger.textContent(), '选择模型，当前 Flash Trial，推理等级 High');
    assert.equal((await writes()).at(-1).value, 'high');
    await invoke(trigger);
    await invoke(root.getByRole('menuitem', { name: /推理等级，当前/ }));
    await root.getByRole('menuitemradio', { name: 'High', checked: true }).waitFor();
    await page.keyboard.press('Escape');
    await root.getByRole('menuitem', { name: /推理等级，当前 High/ }).waitFor();
    await page.keyboard.press('Escape');
    await waitExpanded(false);
    assert.ok(await isFocused(trigger));
    await invoke(trigger);
    await waitExpanded(true);
    const outside = page.getByRole('button', { name: '搜索会话', exact: true });
    await outside.focus();
    await waitExpanded(false);
    assert.ok(await isFocused(outside));
  } else if (mode === 'model-catalog') {
    const state = () => page.evaluate(() => ({
      reads: window.__verification.calls.filter(call => call.name === 'model_catalog').length,
      writes: window.__verification.calls.filter(call => call.name === 'set_config_option').map(call => call.args),
    }));
    const waitRead = count => page.waitForFunction(count =>
      window.__verification.calls.filter(call => call.name === 'model_catalog').length === count
        && typeof window.__verification.settleCatalog === 'function', count, { timeout: 5000 });
    const waitWrite = async (count, route) => {
      await page.waitForFunction(count => window.__verification.calls
        .filter(call => call.name === 'set_config_option').length === count
        && Boolean(window.__verification.settleModelChoice), count, { timeout: 5000 });
      assert.deepEqual((await state()).writes.at(-1), {
        sessionId, configId: 'model', value: JSON.stringify(route),
      });
    };
    const settle = scenario => page.evaluate(scenario => window.__verification.settleCatalog(scenario), scenario);
    const shot = name => page.screenshot({ path: join(artifacts, `${name}.png`) });
    const openChoices = async () => {
      await page.mouse.click(1140, 1039);
      await page.waitForTimeout(190);
      await page.keyboard.press('ArrowDown');
      await page.keyboard.press('Enter');
      await page.waitForFunction(() => document.activeElement instanceof HTMLTextAreaElement
        && !document.activeElement.readOnly, null, { timeout: 5000 });
    };
    const query = async text => {
      await page.keyboard.press('ControlOrMeta+a');
      await page.keyboard.type(text);
    };
    const retryFromEmpty = async () => {
      await page.keyboard.press('Tab');
      await page.keyboard.press('Enter');
    };

    await waitRead(1);
    await settle('error');
    await page.waitForTimeout(200);
    await openChoices();
    // Opening a failed catalog already starts the reload. The last ACP
    // options stay available while the directory error is retried.
    await waitRead(2);
    await shot('loading');
    await settle('error');
    await page.waitForTimeout(200);
    await query('ghost');
    await shot('catalog-error');
    await retryFromEmpty();
    await waitRead(3);
    await page.evaluate(() => {
      for (let i = 0; i < 3; i++) window.__verification.emit({ kind: 'model_catalog_changed' });
    });
    assert.equal((await state()).reads, 3, 'one catalog read at a time');
    await settle('stale');
    await waitRead(4);
    await page.keyboard.press('Enter');
    assert.deepEqual((await state()).writes, [], 'the invalidated catalog never supplies a selectable model');

    await settle('partial');
    await page.waitForTimeout(200);
    await query('matrix-fast');
    await shot('partial-failure');
    await page.keyboard.press('Enter');
    await waitWrite(1, ['deepseek-official', 'official-matrix-fast']);
    await page.evaluate(() => window.__verification.settleModelChoice(null));
    await page.waitForTimeout(200);
    await openChoices();
    assert.equal((await state()).reads, 4, 'both entries reuse the ready directory');
    await query('routed telemetry');
    await page.keyboard.press('Enter');
    assert.equal((await state()).writes.length, 1, 'the failed provider is unavailable for new selections');
    await retryFromEmpty();
    await waitRead(5);
    await settle('ready');
    await page.waitForTimeout(220);
    await shot('recovered-provider');
    await page.keyboard.press('Enter');
    await waitWrite(2, ['custom-host', 'replay']);
    await page.evaluate(() => window.__verification.settleModelChoice(null));
    await page.waitForTimeout(200);

    await page.mouse.click(630, 984);
    await page.keyboard.type('/model');
    await page.keyboard.press('Enter');
    await page.waitForTimeout(190);
    await page.keyboard.type('Flash Trial');
    await page.keyboard.press('Enter');
    await waitWrite(3, ['deepseek-official', 'official-matrix-fast']);
    assert.equal((await state()).reads, 5, '/model reused the recovered catalog');
    await page.evaluate(() => window.__verification.settleModelChoice(null));
    await page.waitForTimeout(200);

    await page.evaluate(() => window.__verification.emit({ kind: 'model_catalog_changed' }));
    await waitRead(6);
    await settle('error');
    await page.waitForTimeout(180);
    await page.mouse.click(630, 984);
    await page.keyboard.type('/model');
    await page.keyboard.press('Enter');
    await waitRead(7);
    await settle('error');
    await page.waitForTimeout(180);
    await query('ghost');
    await retryFromEmpty();
    await waitRead(8);
    await query('Long Reasoner');
    await page.keyboard.press('Enter');
    await waitWrite(4, ['deepseek-official', 'native-9a']);
    await page.evaluate(() => window.__verification.settleModelChoice(null));
    await settle('ready');
    await page.waitForTimeout(200);
  } else if (mode === 'attachments') {
    page.setDefaultTimeout(9000);
    const queue = page.getByRole('group', { name: '排队消息' });
    const drafts = page.getByRole('group', { name: '草稿附件' });
    const files = page.getByRole('group', { name: '会话文件' });
    const root = page.locator('#composer-submit-accessibility');
    const clicks = name => root.getByRole('button', { name, exact: true });
    const waitCalls = (name, count) => page.waitForFunction(({ name, count }) =>
      window.__verification.calls.filter(call => call.name === name).length >= count, { name, count });
    const send = async name => {
      await page.waitForFunction(name => [...document.querySelectorAll('#composer-submit-accessibility button')]
        .some(button => !button.hidden && !button.disabled && button.textContent === name), name);
      await clicks(name).dispatchEvent('click');
    };
    const drop = blobs => page.evaluate(blobs => {
      const dataTransfer = new DataTransfer();
      for (const { name, size, value } of blobs) {
        const bytes = new Uint8Array(size).fill(value);
        if (bytes.length) bytes[bytes.length - 1] = value + 1;
        dataTransfer.items.add(new File([bytes], name, { type: 'application/octet-stream' }));
      }
      window.dispatchEvent(new DragEvent('drop', { dataTransfer, bubbles: true, cancelable: true }));
    }, blobs);
    const operations = () => page.evaluate(() => window.__verification.calls.filter(call =>
      call.name === 'stage_prompt_file').map(({ args: { operation } }) => ({
        kind: operation.kind, name: operation.name, size: operation.size, id: operation.id,
        offset: operation.offset, length: operation.chunk?.length, first: operation.chunk?.[0], last: operation.chunk?.at(-1),
      })));
    const large = 256 * 1024 + 3;
    await drop([{ name: 'same-name.bin', size: large, value: 18 }, { name: 'same-name.bin', size: large, value: 90 }]);
    const draftFiles = drafts.getByRole('button', { name: '移除附件 same-name.bin', exact: true });
    await draftFiles.last().waitFor();
    assert.equal(await draftFiles.count(), 2);
    await page.evaluate(() => { window.__verification.failFileChunk = true; });
    await send('发送');
    await waitCalls('stage_prompt_file', 4);
    const failed = await operations();
    assert.deepEqual(failed.map(operation => operation.kind), ['begin', 'append', 'append', 'discard']);
    assert.equal(await draftFiles.count(), 2);
    assert.equal(await page.evaluate(() => window.__verification.calls.some(call => call.name === 'queue_prompt')), false);
    await send('发送');
    await waitCalls('queue_prompt', 1);
    const staged = (await operations()).slice(failed.length).filter(operation => operation.kind === 'append');
    assert.deepEqual(staged.map(({ offset, length, first, last }) => [offset, length, first, last]), [
      [0, 256 * 1024, 18, 18], [256 * 1024, 3, 18, 19],
      [0, 256 * 1024, 90, 90], [256 * 1024, 3, 90, 91],
    ]);
    const intent = await page.evaluate(() => window.__verification.calls.find(call => call.name === 'queue_prompt').args);
    assert.equal(intent.mode, 'queue');
    assert.deepEqual(intent.prompt, [{ type: 'file', uploadId: 'uploaded-2' }, { type: 'file', uploadId: 'uploaded-3' }]);
    await queue.getByRole('status').getByText('发送中…').waitFor();
    await page.evaluate(() => window.__verification.acceptQueue());
    await queue.getByText(/排队文件 same-name.bin/).first().waitFor();
    assert.equal(await queue.getByText(/排队文件 same-name.bin/).count(), 2);
    assert.equal(await queue.getByRole('button', { name: '编辑排队消息' }).isDisabled(), true);
    await page.evaluate(() => window.__verification.admitQueue());
    await page.evaluate(() => window.__verification.settleQueue());
    await queue.waitFor({ state: 'hidden' });
    await waitCalls('stage_prompt_file', 12);
    assert.equal((await operations()).filter(operation => operation.kind === 'discard').length, 3);
    const downloadButtons = files.getByRole('button', { name: '下载附件 same-name.bin', exact: true });
    await downloadButtons.first().waitFor();
    await page.screenshot({ path: join(artifacts, 'logged-file-cards.png') });
    // This AX surface is offscreen; use the painted card in the fixture screenshot.
    await page.mouse.click(920, 250);
    await waitCalls('download_user_attachment', 1);
    assert.deepEqual(await page.evaluate(() => window.__verification.calls.find(call =>
      call.name === 'download_user_attachment').args), {
      attachmentId: 'sha256:' + '2'.padStart(64, '0'), name: 'same-name.bin', bytes: large,
    });
    assert.equal(await downloadButtons.count(), 2);
    await downloadButtons.last().dispatchEvent('click');
    await waitCalls('download_user_attachment', 2);
    assert.deepEqual(await page.evaluate(() => window.__verification.calls.filter(call =>
      call.name === 'download_user_attachment').at(-1).args), {
      attachmentId: 'sha256:' + '3'.padStart(64, '0'), name: 'same-name.bin', bytes: large,
    });
    await page.evaluate(sessionId => window.__verification.emit({
      kind: 'session_update', sessionId,
      update: { sessionUpdate: 'gpui_turn_tail', type: 'turn/start', turn: 2, seq: 30 },
    }), sessionId);
    await drop([{ name: 'empty.bin', size: 0, value: 0 }]);
    await send('排队发送');
    await waitCalls('queue_prompt', 2);
    assert.deepEqual((await operations()).at(-1)?.kind, 'begin');
    await page.evaluate(() => window.__verification.settleQueue());
    await queue.getByText(/排队文件 empty.bin/).waitFor();

    await waitCalls('stage_prompt_file', 14);
    await drop([{ name: 'command-reference.bin', size: 3, value: 111 }]);
    const modelControls = page.locator('#model-accessibility');
    const launcher = modelControls.getByRole('button', { name: '调用指令', exact: true });
    const suggestions = modelControls.getByRole('listbox', { name: '触发候选建议' });
    const editor = page.locator('#gpui-text-input');
    await launcher.dispatchEvent('click');
    await suggestions.getByRole('option', { name: /^计划 \/plan/ }).waitFor();
    await page.keyboard.press('Escape');
    await suggestions.waitFor({ state: 'hidden' });
    await page.waitForFunction(() => document.activeElement?.id === 'gpui-text-input' &&
      document.activeElement.hasAttribute('aria-label'));
    await editor.fill('/plan off');
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '/plan off');
    await page.keyboard.press('Enter');
    await waitCalls('execute_session_command', 1);
    await waitCalls('stage_prompt_file', 17);
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '/plan off');
    await editor.fill('/计划 check this reference');
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '/计划 check this reference');
    await page.keyboard.press('Enter');
    await waitCalls('execute_session_command', 2);
    const commands = await page.evaluate(() => window.__verification.calls
      .filter(call => call.name === 'execute_session_command').map(call => call.args));
    assert.deepEqual(commands.map(args => [args.line, args.attachments]), [
      ['/plan off', [{ type: 'file', uploadId: 'uploaded-5' }]],
      ['/plan check this reference', [{ type: 'file', uploadId: 'uploaded-6' }]],
    ]);
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '');
    assert.equal((await operations()).filter(operation => operation.kind === 'discard').length, 6);
    await page.screenshot({ path: join(artifacts, 'plan-file-command.png') });
    const beforeRemoval = (await operations()).length;
    await drop([{ name: 'remove-me.bin', size: 2, value: 7 }]);
    const remove = drafts.getByRole('button', { name: '移除附件 remove-me.bin', exact: true });
    await remove.waitFor();
    await remove.dispatchEvent('click');
    await remove.waitFor({ state: 'hidden' });
    assert.equal((await operations()).length, beforeRemoval);
  } else if (mode === 'composer-focus') {
    const model = page.getByRole('button', { name: /选择模型|请选择模型/ });
    const proxy = page.getByRole('textbox', { name: '消息输入框', exact: true });
    const editor = page.locator('#gpui-text-input');
    const closeModel = async () => {
      await model.evaluate(node => node.click());
      await page.locator('#model-accessibility-pane').waitFor({ state: 'visible' });
      await model.evaluate(node => node.click());
      await page.locator('#model-accessibility-pane').waitFor({ state: 'hidden' });
      await proxy.waitFor();
    };
    await closeModel();
    assert.equal(await editor.getAttribute('aria-hidden'), 'true', 'An unbound IME conduit is not an editable accessibility field');
    await proxy.focus();
    await page.waitForFunction(() => document.activeElement?.id === 'gpui-text-input' && document.activeElement.getAttribute('aria-label'));
    await editor.fill('Native composer focus restored');
    await page.getByRole('button', { name: '发送', exact: true }).waitFor();
    await page.waitForFunction(() => !document.querySelector('#composer-submit-accessibility button').disabled);
    await closeModel();
    assert.equal(await proxy.inputValue(), 'Native composer focus restored');
    await proxy.evaluate(node => {
      node.value = 'Native value replacement'; node.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertReplacementText' }));
    });
    await page.waitForFunction(() => document.activeElement?.id === 'gpui-text-input' && document.activeElement.value === 'Native value replacement');
    await proxy.waitFor({ state: 'hidden' });
    assert.notEqual(await editor.getAttribute('aria-hidden'), 'true');
    await page.getByRole('button', { name: '发送', exact: true }).evaluate(node => node.click());
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'prompt' && JSON.stringify(call.args).includes('Native value replacement')));
    await page.screenshot({ path: join(artifacts, 'native-composer-submission.png') });
    assert.equal(errors.length, 0, `no browser errors: ${errors.join('\n')}`);
  } else if (mode === 'composer') {
    page.setDefaultTimeout(9000);
    const root = page.locator('#composer-submit-accessibility');
    const setting = root.getByRole('group', { name: '繁忙时的发送行为' });
    const trigger = mode => setting.getByRole('button', { name: `繁忙时的发送行为，${mode}` });
    const editor = page.locator('#gpui-text-input');
    const queue = page.getByRole('group', { name: '排队消息' });
    const writes = name => page.evaluate(name => window.__verification.calls.filter(call => call.name === name), name);
    const waitForReply = name => page.waitForFunction(name => typeof window.__verification[name] === 'function', name);
    const settle = (name, error = null) => page.evaluate(({ name, error }) => window.__verification[name](error), { name, error });
    const click = locator => locator.dispatchEvent('click');
    const fill = async text => {
      await page.mouse.click(618, 984);
      await page.waitForFunction(() => document.activeElement?.id === 'gpui-text-input' &&
        document.activeElement.getAttribute('aria-label'), null, { timeout: 5000 });
      await editor.fill(text);
      await page.waitForFunction(text => document.getElementById('gpui-text-input')?.value === text, text);
    };
    const send = async label => {
      await page.waitForFunction(label => [...document.querySelectorAll('#composer-submit-accessibility button')]
        .some(button => !button.hidden && !button.disabled && button.textContent === label), label);
      await click(root.getByRole('button', { name: label, exact: true }));
    };
    const select = async (current, choice) => {
      await click(trigger(current));
      await click(setting.getByRole('menuitemradio', { name: choice }));
      await waitForReply('settleComposer');
    };

    await click(root.getByRole('button', { name: '更多设置', exact: true }));
    await click(root.getByRole('button', { name: '设置', exact: true }));
    await click(page.locator('#settings-shell').getByRole('button', { name: '通用设置', exact: true }));
    await trigger('排队发送').waitFor();
    await select('排队发送', '插话发送');
    await trigger('插话发送').waitFor();
    await settle('settleComposer', 'Fixture preference save failed');
    await setting.getByRole('alert').getByText('Fixture preference save failed').waitFor();
    await trigger('排队发送').waitFor();
    await select('排队发送', '插话发送');
    await settle('settleComposer');
    await trigger('插话发送').waitFor();
    await trigger('插话发送').press('Escape');
    await setting.waitFor({ state: 'hidden' });
    await page.reload();
    await page.waitForFunction(() => document.querySelector('canvas') && !document.getElementById('boot'), null, { timeout: 60000 });
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'turn_token_usage'),
      null, { timeout: 12000 });

    await fill('start a turn');
    await send('发送');
    await waitForReply('completePrompt');
    await fill('plain Enter steers');
    await page.keyboard.press('Enter');
    await waitForReply('settleQueue');
    assert.equal((await writes('queue_prompt')).at(-1).args.mode, 'steer');
    await settle('settleQueue');
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '');
    await fill('primary button steers');
    await send('插话发送');
    await waitForReply('settleQueue');
    assert.equal((await writes('queue_prompt')).at(-1).args.mode, 'steer');
    await settle('settleQueue');
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '');
    await fill('accelerated queues');
    await page.keyboard.press('Meta+Enter');
    await waitForReply('settleQueue');
    assert.equal((await writes('queue_prompt')).at(-1).args.mode, 'queue');
    await settle('settleQueue');
    await queue.getByText('accelerated queues').waitFor();

    await page.evaluate(() => window.__verification.addInboxFixtures());
    await click(queue.getByRole('button', { name: '3 条排队消息' }));
    await queue.getByRole('img', { name: '排队消息图片' }).waitFor();
    await queue.getByText(/排队文件 fixture-report.pdf/).waitFor();
    const image = queue.getByRole('listitem').filter({ hasText: 'Queued picture' });
    assert.equal(await image.getByRole('button', { name: '编辑排队消息' }).isDisabled(), true);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'read_attachment'));
    await page.waitForTimeout(300);
    assert.equal((await writes('read_attachment')).length, 1);
    await page.screenshot({ path: join(artifacts, 'queue-attachments.png') });
    await page.evaluate(() => window.__verification.admitInboxPicture());
    await image.waitFor({ state: 'hidden' });
    assert.equal((await writes('read_attachment')).length, 1, 'transcript admission reuses the picture');

    assert.match(await editor.getAttribute('aria-label'), /插话发送全部排队消息/);
    await editor.focus();
    await page.keyboard.press('Meta+Enter');
    await waitForReply('settleInboxAction');
    const list = () => writes('inbox_action').then(calls => calls.map(call => call.args));
    assert.deepEqual((await list()).map(item => [item.itemId, item.action]), [['queue-3', 'steer']]);
    await settle('settleInboxAction');
    await waitForReply('settleInboxAction');
    assert.deepEqual((await list()).map(item => [item.itemId, item.action]), [
      ['queue-3', 'steer'], ['queued-file', 'steer'],
    ]);
    await settle('settleInboxAction');
    await queue.waitFor({ state: 'hidden' });
  } else if (mode === 'queue') {
    page.setDefaultTimeout(8000);
    const queue = page.getByRole('group', { name: '排队消息' });
    const composer = page.locator('#gpui-text-input');
    const editor = queue.getByRole('textbox', { name: '编辑排队消息' });
    const writes = name => page.evaluate(name => window.__verification.calls.filter(call => call.name === name), name);
    const waitForReply = name => page.waitForFunction(name => typeof window.__verification[name] === 'function', name, { timeout: 6500 });
    const settle = (name, error = null) => page.evaluate(({ name, error }) => window.__verification[name](error), { name, error });
    const click = locator => locator.dispatchEvent('click');
    const type = async text => {
      const focused = () => document.activeElement === document.getElementById('gpui-text-input') &&
        !document.activeElement.readOnly && document.activeElement.hasAttribute('aria-label');
      for (let attempt = 0; attempt < 3 && !await page.evaluate(focused); attempt += 1) {
        await page.mouse.move(618, 984);
        await page.mouse.click(618, 984);
        await page.waitForFunction(focused, null, { timeout: 900 }).catch(() => {});
      }
      await page.waitForFunction(focused, null, { timeout: 5000 });
      await composer.fill(text);
      await page.waitForFunction(text => document.getElementById('gpui-text-input')?.value === text,
        text, { timeout: 5000 });
    };

    await type('active turn');
    await page.keyboard.press('Enter');
    await waitForReply('completePrompt');
    await type('queue item one');
    await page.keyboard.press('Enter');
    await page.keyboard.press('Enter');
    await waitForReply('settleQueue');
    await queue.getByRole('status').getByText('发送中…').waitFor();
    assert.equal((await writes('queue_prompt')).length, 1);
    assert.equal((await writes('queue_prompt'))[0].args.mode, 'queue');
    await page.evaluate(() => window.__verification.acceptQueue());
    await queue.getByRole('button', { name: '编辑排队消息' }).waitFor();
    assert.equal(await queue.getByRole('listitem').count(), 1, 'the Host echo and local pending row must merge');
    await settle('settleQueue');
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '');
    await type('queue item two');
    await page.keyboard.press('Enter');
    await waitForReply('settleQueue');
    await settle('settleQueue');
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '');
    const summary = queue.getByRole('button', { name: '2 条排队消息' });
    await summary.waitFor();
    assert.equal(await summary.getAttribute('aria-expanded'), 'false');
    assert.equal(await queue.getByRole('listitem').count(), 0);
    await click(summary);
    const first = queue.getByRole('listitem').filter({ hasText: 'queue item one' });
    const second = queue.getByRole('listitem').filter({ hasText: 'queue item two' });
    await first.waitFor();
    await second.waitFor();
    await page.screenshot({ path: join(artifacts, 'queue-expanded.png') });
    await type('composer draft');
    await click(first.getByRole('button', { name: '编辑排队消息' }));
    await editor.waitFor();
    assert.equal(await editor.inputValue(), 'queue item one');
    await editor.fill('modified first');
    await editor.press('Shift+Enter');
    await page.keyboard.type('continuation');
    await editor.press('Enter');
    await waitForReply('settleInboxAction');
    assert.equal(await editor.getAttribute('readonly'), '');
    await settle('settleInboxAction', 'Fixture save rejected');
    await queue.getByRole('alert').getByText('Fixture save rejected').waitFor();
    assert.equal(await editor.inputValue(), 'modified first\ncontinuation');
    assert.equal(await composer.inputValue(), 'composer draft');
    assert.equal((await writes('inbox_action')).length, 1);

    await page.evaluate(() => window.__verification.requestInboxSnapshot());
    await waitForReply('releaseInboxSnapshot');
    await editor.press('Enter');
    await waitForReply('settleInboxAction');
    assert.deepEqual((await writes('inbox_action')).at(-1).args, {
      sessionId, itemId: 'queue-1', action: 'edit', text: 'modified first\ncontinuation',
    });
    await settle('settleInboxAction');
    const updated = queue.getByRole('listitem').filter({ hasText: 'modified first' });
    await updated.waitFor();
    await page.evaluate(() => window.__verification.releaseInboxSnapshot());
    assert.deepEqual(await queue.getByRole('listitem').allTextContents().then(rows =>
      rows.map(text => text.startsWith('modified first') ? 'first' : 'second')), ['first', 'second']);
    await click(updated.getByRole('button', { name: '编辑排队消息' }));
    await editor.fill('discard this');
    await editor.press('Escape');
    await updated.waitFor();
    assert.equal((await writes('inbox_action')).length, 2);
    await click(updated.getByRole('button', { name: '插话发送' }));
    await waitForReply('settleInboxAction');
    assert.equal((await writes('inbox_action')).at(-1).args.action, 'steer');
    await settle('settleInboxAction');
    await updated.waitFor({ state: 'hidden' });
    await click(second.getByRole('button', { name: '删除排队消息' }));
    await waitForReply('settleInboxAction');
    assert.equal((await writes('inbox_action')).at(-1).args.itemId, 'queue-2');
    await settle('settleInboxAction');
    await queue.waitFor({ state: 'hidden' });
    assert.equal(await composer.inputValue(), 'composer draft');
    await composer.focus();
    await page.keyboard.press('Meta+Enter');
    await waitForReply('settleQueue');
    assert.equal((await writes('queue_prompt')).at(-1).args.mode, 'steer');
    await settle('settleQueue', 'Fixture admission rejected');
    assert.equal(await composer.inputValue(), 'composer draft');
    await page.keyboard.press('Enter');
    await waitForReply('settleQueue');
    await queue.getByRole('status').getByText('发送中…').waitFor();
    await page.evaluate(() => window.__verification.acceptQueue());
    await queue.getByRole('button', { name: '编辑排队消息' }).waitFor();
    await page.evaluate(() => window.__verification.admitQueue());
    await queue.waitFor({ state: 'hidden' });
    await settle('settleQueue');
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '');
    assert.equal(await queue.isHidden(), true, 'a late submission reply must not bring back an admitted row');
  } else if (mode === 'approval') {
    const card = page.getByRole('group', { name: '工具审批' });
    const details = card.getByRole('group', { name: '审批详情' });
    const allow = card.getByRole('button', { name: '允许一次', exact: true });
    const reject = card.getByRole('button', { name: '拒绝', exact: true });
    const always = card.getByRole('button', { name: '始终允许', exact: true });
    const choices = [
      { optionId: 'persist-5', kind: 'allow_always', name: 'Always' },
      { optionId: 'decline-3', kind: 'reject_once', name: 'Decline' },
      { optionId: 'persist-2', kind: 'reject_always', name: 'Always decline' },
      { optionId: 'accept-9', kind: 'allow_once', name: 'Accept' },
    ];
    const add = (requestId, options = choices) => page.evaluate(
      event => window.__verification.emit(event), {
        kind: 'permission', sessionId, requestId, options,
        toolCall: { toolCallId: requestId, title: 'bash', rawInput: { command: `printf '${requestId}'` } },
      });
    const close = requestId => page.evaluate(
      event => window.__verification.emit(event), { kind: 'permission_closed', sessionId, requestId });
    const current = id => page.waitForFunction(
      id => document.getElementById('approval-accessibility')?.dataset.approvalKey === id, id);
    const writes = () => page.evaluate(() => window.__verification.calls.filter(call =>
      call.name === 'respond_permission').map(call => call.args));
    const waitWrite = async (count, requestId, optionId) => {
      await page.waitForFunction(count => window.__verification.calls.filter(call =>
        call.name === 'respond_permission').length >= count, count, { timeout: 5000 });
      assert.equal((await writes()).length, count);
      assert.deepEqual((await writes()).at(-1), { sessionId, requestId, optionId });
    };
    const settle = failure => page.evaluate(
      failure => window.__verification.settleApproval(failure), failure);
    const dispatchKey = key => details.evaluate((node, init) => {
      node.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }));
    }, key);

    await add('first');
    await add('second');
    await add('first'); // Repeated delivery of a pending request is idempotent.
    await current('first');
    await card.getByRole('status').getByText('等待审批').waitFor();
    await details.getByText("printf 'first'", { exact: true }).waitFor();
    assert.deepEqual(await card.getByRole('button').allTextContents(), ['拒绝', '始终拒绝', '允许一次', '始终允许']);
    await page.locator('canvas').first().evaluate(node => { node.tabIndex = 0; node.focus(); });
    await page.keyboard.press('Enter');
    await page.keyboard.press('Escape');
    assert.deepEqual(await writes(), []);
    await details.focus();
    await dispatchKey({ key: 'Enter', isComposing: true });
    await dispatchKey({ key: 'Enter', repeat: true });
    await dispatchKey({ key: 'Enter', shiftKey: true });
    await details.dispatchEvent('compositionend');
    await page.keyboard.press('Enter');
    assert.deepEqual(await writes(), []);

    await page.screenshot({ path: join(artifacts, 'approval-waiting.png') });
    await page.keyboard.press('Enter');
    await waitWrite(1, 'first', 'accept-9');
    await card.getByRole('status').getByText('正在提交审批…').waitFor();
    assert.equal(await card.getAttribute('aria-busy'), 'true');
    assert.equal(await allow.isDisabled(), true);
    assert.equal(await reject.isDisabled(), true);
    await page.keyboard.press('Escape');
    await always.dispatchEvent('click');
    assert.equal((await writes()).length, 1);
    await settle('Fixture write failure.');
    await card.getByRole('alert').getByText(/提交审批失败，请重试。Fixture write failure/).waitFor();
    assert.equal(await card.getAttribute('aria-busy'), 'false');
    await details.getByText("printf 'first'", { exact: true }).waitFor();

    await allow.focus();
    await page.keyboard.press('Enter');
    await waitWrite(2, 'first', 'accept-9');
    await settle(null);
    await current('second');
    await details.getByText("printf 'second'", { exact: true }).waitFor();
    assert.equal(await details.evaluate(node => node === document.activeElement), true);
    await page.keyboard.press('Escape');
    await waitWrite(3, 'second', 'decline-3');
    await add('third', [choices[0], choices[2]]);
    await close('second'); // The host can withdraw a request while its reply is in flight.
    await current('third');
    await settle('Fixture obsolete failure.');
    await close('first'); // A stale close cannot dismiss the new card.
    await details.getByText("printf 'third'", { exact: true }).waitFor();
    assert.equal(await card.getByRole('alert').count(), 0);
    await page.keyboard.press('Enter');
    await page.keyboard.press('Escape');
    assert.equal((await writes()).length, 3, 'keyboard defaults must never choose persistent decisions');

    await always.focus();
    await page.keyboard.press('Enter');
    await waitWrite(4, 'third', 'persist-5');
    await settle('permission/request-closed');
    await card.waitFor({ state: 'hidden' });
    assert.equal((await writes()).length, 4);

    await add('canvas');
    await current('canvas');
    // Click the painted details in the fixed-size fixture window, outside the
    // accessibility mirror, and use the GPUI keyboard path.
    await page.mouse.click(650, 980);
    await page.keyboard.press('Shift+Enter');
    await page.waitForTimeout(120);
    assert.equal((await writes()).length, 4, 'modified Enter must not approve on the canvas');
    await page.evaluate(() => document.activeElement.dispatchEvent(
      new CompositionEvent('compositionend', { bubbles: true })));
    await page.keyboard.press('Enter');
    await page.waitForTimeout(120);
    assert.equal((await writes()).length, 4, 'IME completion must not approve on the canvas');
    await page.keyboard.press('Escape');
    await waitWrite(5, 'canvas', 'decline-3');
    await settle(null);
    await card.waitFor({ state: 'hidden' });

    await add('pre-reconnect');
    await current('pre-reconnect');
    await page.evaluate(() => window.__verification.emit({ kind: 'service', status: 'starting' }));
    await card.waitFor({ state: 'hidden' });
    assert.equal((await writes()).length, 5);
  } else if (mode === 'permission') {
    const root = page.locator('#model-accessibility');
    const launcher = root.getByRole('button', { name: '调用指令' });
    const suggestions = root.getByRole('listbox', { name: '触发候选建议' });
    const search = root.getByRole('textbox', { name: '筛选选项' });
    const choices = root.getByRole('listbox', { name: '/permission 匹配项' });
    const risk = root.getByRole('dialog');
    const acknowledge = risk.getByRole('checkbox');
    const invoke = locator => locator.dispatchEvent('click');
    const waitFocus = locator => locator.evaluate(node => node === document.activeElement);
    const settle = async failure => {
      await page.waitForFunction(() => typeof window.__verification.settlePermission === 'function', null, { timeout: 5000 });
      await page.evaluate(failure => window.__verification.settlePermission(failure), failure);
    };
    const writes = () => page.evaluate(() => window.__verification.calls.filter(call =>
      call.name === 'permission_mode' && call.args.preset).map(call => call.args));
    const selectRisk = async (text, option, title) => {
      await search.fill(text);
      await invoke(choices.getByRole('option', { name: option }));
      await risk.getByRole('heading', { name: title }).waitFor();
      assert.ok(await waitFocus(acknowledge));
      assert.equal(await acknowledge.isChecked(), false);
    };

    await invoke(launcher);
    await suggestions.waitFor();
    await page.keyboard.type('/permission');
    await invoke(suggestions.getByRole('option', { name: /^权限 \/permission/, selected: true }));
    await search.waitFor();
    assert.ok(await waitFocus(search));
    await settle('Fixture permission read failed');
    await root.getByText('Fixture permission read failed').waitFor();
    await search.press('Tab');
    const retry = root.getByRole('button', { name: '重试加载权限' });
    assert.ok(await waitFocus(retry));
    await retry.press('Enter');
    await settle(null);
    await choices.getByRole('option', { name: '仅可查看，当前模式' }).waitFor();

    await selectRisk('自动', /^自动审查 EXP/, '确认启用自动审查（实验）？');
    await risk.getByText(/自动审查不使用沙箱/).waitFor();
    const autoConfirm = risk.getByRole('button', { name: '启用自动审查' });
    assert.ok(await autoConfirm.isDisabled());
    await invoke(autoConfirm);
    assert.deepEqual(await writes(), []);
    await acknowledge.press('Tab');
    assert.ok(await waitFocus(risk.getByRole('button', { name: '取消' })));
    await page.keyboard.press('Escape');
    await risk.waitFor({ state: 'hidden' });
    assert.ok(await waitFocus(search));
    assert.equal(await search.inputValue(), '自动');

    const fullConfirm = risk.getByRole('button', { name: '启用完全权限' });
    await selectRisk('完全', '完全权限', '确认启用完全权限？');
    await page.screenshot({ path: join(artifacts, 'risk.png') });
    await acknowledge.press('Space');
    await page.waitForFunction(() => ![...document.querySelectorAll('#permission-risk-accessibility button')].at(-1)?.disabled, null, { timeout: 5000 });
    await invoke(fullConfirm);
    await settle('Fixture permission write failed');
    await root.getByText('Fixture permission write failed').waitFor();
    assert.ok(await waitFocus(search));
    assert.equal(await search.inputValue(), '完全');
    await selectRisk('完全', '完全权限', '确认启用完全权限？');
    await acknowledge.press('Space');
    await page.waitForFunction(() => ![...document.querySelectorAll('#permission-risk-accessibility button')].at(-1)?.disabled, null, { timeout: 5000 });
    await invoke(fullConfirm);
    await settle(null);
    const full = root.getByRole('button', { name: '访问模式，当前：完全权限' });
    await full.waitFor();
    await page.waitForFunction(() => document.activeElement?.id === 'gpui-text-input', null, { timeout: 5000 });
    await page.waitForFunction(() => document.getElementById('gpui-text-input')?.value === '', null, { timeout: 5000 });

    await invoke(full);
    await invoke(root.getByRole('menuitemradio', { name: '完全权限', checked: true }));
    await risk.waitFor({ state: 'hidden' });
    assert.equal((await writes()).length, 2);
    await invoke(full);
    await invoke(root.getByRole('menuitemradio', { name: '工作区内修改' }));
    await settle(null);
    const workspace = root.getByRole('button', { name: '访问模式，当前：工作区内修改' });
    await workspace.waitFor();
    assert.ok(await waitFocus(workspace));
    await invoke(workspace);
    await invoke(root.getByRole('menuitemradio', { name: '自动审查 EXP' }));
    await autoConfirm.waitFor();
    assert.ok(await autoConfirm.isDisabled());
    await acknowledge.press('Space');
    await page.waitForFunction(() => ![...document.querySelectorAll('#permission-risk-accessibility button')].at(-1)?.disabled, null, { timeout: 5000 });
    await invoke(autoConfirm);
    await settle(null);
    const auto = root.getByRole('button', { name: '访问模式，当前：自动审查' });
    await auto.waitFor();
    assert.ok(await waitFocus(auto));
    assert.deepEqual(await writes(), [
      { sessionId, preset: 'danger-full-access', acknowledged: true },
      { sessionId, preset: 'danger-full-access', acknowledged: true },
      { sessionId, preset: 'workspace-write', acknowledged: false },
      { sessionId, preset: 'auto', acknowledged: true },
    ]);
  } else if (mode === 'team') {
    const root = page.locator('#model-accessibility');
    const trigger = root.locator(':scope > button');
    const teamSwitch = root.getByRole('switch', { name: '开启智能体团队协作' });
    await page.waitForTimeout(700);
    await page.screenshot({ path: join(artifacts, 'board.png') });
    await page.mouse.click(1120, 1039);
    await root.getByRole('menuitem', { name: '智能体团队协作，当前 关闭' }).waitFor();
    await page.screenshot({ path: join(artifacts, 'menu.png') });
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('Enter');
    await teamSwitch.waitFor();
    await page.screenshot({ path: join(artifacts, 'pane.png') });
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'team_state' && call.args.enabled === true), null, { timeout: 5000 });
    await page.waitForTimeout(500);
    await page.screenshot({ path: join(artifacts, 'enabled.png') });
    const dialog = root.getByRole('dialog', { name: '确认关闭智能体团队协作？' });
    const cancel = dialog.getByRole('button', { name: '取消', exact: true });
    const close = dialog.getByRole('button', { name: '关闭', exact: true });
    const confirm = dialog.getByRole('button', { name: '确认关闭', exact: true });
    const second = 'team-second-session';
    const writes = () => page.evaluate(() => window.__verification.calls
      .filter(call => call.name === 'team_state' && typeof call.args.enabled === 'boolean')
      .map(({ args }) => [args.sessionId, args.enabled]));
    const activate = locator => locator.dispatchEvent('click');
    const focus = role => page.waitForFunction(role => document.activeElement?.textContent === role, role, { timeout: 5000 });
    const waitChecked = value => page.waitForFunction(value => document.querySelector('#model-accessibility [role=switch]')
      ?.getAttribute('aria-checked') === String(value), value, { timeout: 5000 });
    const openTeam = async label => {
      await activate(trigger);
      await activate(root.getByRole('menuitem', { name: `智能体团队协作，当前 ${label}` }));
      await teamSwitch.waitFor();
    };
    const settle = async (session, failure) => {
      await page.waitForFunction(id => typeof window.__verification.teamReplies?.[id] === 'function', session, { timeout: 5000 });
      await page.evaluate(({ session, failure }) => window.__verification.teamReplies[session](failure), { session, failure });
    };
    const reopenDialog = async () => {
      await openTeam('开启');
      await activate(teamSwitch);
      await dialog.waitFor();
      await focus('取消');
    };

    // A session with no model usage can turn Team off immediately.
    await waitChecked(true);
    await teamSwitch.press('Enter');
    await waitChecked(false);
    assert.equal(await dialog.count(), 0);
    assert.deepEqual(await writes(), [[sessionId, true], [sessionId, false]]);
    await page.evaluate(() => { window.__verification.deferTeamSave = true; });
    await activate(teamSwitch);
    await page.waitForFunction(id => Boolean(window.__verification.teamReplies?.[id]), sessionId, { timeout: 5000 });
    await activate(page.getByRole('button', { name: '打开 使用过模型的团队会话', exact: true }));
    await page.waitForFunction(id => window.__verification.calls.some(call => call.name === 'team_state' && call.args.sessionId === id), second, { timeout: 5000 });
    await reopenDialog();
    await page.screenshot({ path: join(artifacts, 'confirm.png') });
    await dialog.getByText(/停止共享任务工具/).waitFor();
    await dialog.getByText(/影响缓存复用/).waitFor();
    assert.equal(await trigger.isVisible(), false);
    await page.keyboard.press('Shift+Tab');
    await focus('关闭');
    await page.keyboard.press('Shift+Tab');
    await focus('确认关闭');
    await page.keyboard.press('Tab');
    await focus('关闭');
    await settle(sessionId, null);
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.ok(await dialog.isVisible(), 'another session’s save must leave this confirmation open');
    assert.equal((await writes()).some(([id]) => id === second), false);
    await close.press('Escape');
    await dialog.waitFor({ state: 'hidden' });
    await focus('选择模型，当前 fixture-model，推理等级 High');

    await page.mouse.click(1120, 1039);
    await root.getByRole('menuitem', { name: '智能体团队协作，当前 开启' }).waitFor();
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('Enter');
    await teamSwitch.waitFor();
    await page.keyboard.press('Enter');
    await dialog.waitFor();
    assert.ok(await page.evaluate(() => document.activeElement instanceof HTMLTextAreaElement));
    await page.keyboard.press('Enter');
    await dialog.waitFor({ state: 'hidden' });
    assert.equal((await writes()).some(([id]) => id === second), false, 'the canvas also initially focuses Cancel');

    await reopenDialog();
    await page.mouse.click(500, 175);
    await dialog.waitFor({ state: 'hidden' });
    assert.equal((await writes()).some(([id]) => id === second), false);

    await reopenDialog();
    await activate(confirm);
    await dialog.getByRole('status').getByText('正在应用…').waitFor();
    assert.ok(await cancel.isDisabled() && await close.isDisabled() && await confirm.isDisabled());
    await page.keyboard.press('Escape');
    await page.mouse.click(500, 175);
    assert.ok(await dialog.isVisible(), 'Escape and backdrop clicks keep a pending save open');
    await settle(second, 'Fixture team save failed');
    await dialog.getByRole('alert').getByText(/Fixture team save failed/).waitFor();
    await focus('确认关闭');
    await confirm.press('Enter');
    await settle(second, null);
    await dialog.waitFor({ state: 'hidden' });
    await focus('选择模型，当前 fixture-model，推理等级 High');
    await openTeam('关闭');
    await waitChecked(false);
    assert.deepEqual((await writes()).filter(([id]) => id === second), [[second, false], [second, false]]);
  } else if (mode === 'plugins') {
    await page.mouse.click(50, 168);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'plugin_center'), null, { timeout: 10000 });
    await page.waitForTimeout(1500);
    await page.screenshot({ path: join(artifacts, 'plugin-badges.png') });
    await page.mouse.click(560, 200);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings'), null, { timeout: 10000 });
    await page.waitForTimeout(700);
    await page.screenshot({ path: join(artifacts, 'plugin-detail.png') });
  } else if (mode === 'trajectory-pages') {
    const olderCalls = () => page.evaluate(() => window.__verification.calls
      .filter(call => call.name === 'load_session_history' && call.args.beforeSeq != null)
      .map(call => call.args.beforeSeq));
    const recentRows = name => page.screenshot({ path: join(artifacts, `${name}.png`), clip: { x: 400, y: 210, width: 660, height: 420 } });
    const settlePage = reply => page.evaluate(reply => {
      window.__verification.settleOlderPage(reply);
      delete window.__verification.settleOlderPage;
    }, reply);
    const waitForPage = () => page.waitForFunction(() => Boolean(window.__verification.settleOlderPage), null, { timeout: 5000 });

    await page.waitForTimeout(700);
    await clickConversationTab(1318);
    await page.waitForTimeout(500);
    await page.screenshot({ path: join(artifacts, 'opened-trajectory.png') });
    await page.mouse.move(700, 540);
    await page.mouse.wheel(0, -3300);
    await page.waitForTimeout(250);
    const first = await recentRows('beyond-last-100');
    await page.mouse.wheel(0, -135);
    await page.waitForTimeout(250);
    assert.ok(!(await recentRows('still-scrolling-within-loaded-page')).equals(first), 'the loaded ledger still scrolls before its last 100 rows');
    assert.deepEqual(await olderCalls(), [], 'the loaded prefix is reachable without a host request');

    await page.mouse.wheel(0, -9000);
    await waitForPage();
    await page.mouse.wheel(0, -120);
    await page.waitForTimeout(250);
    assert.deepEqual(await olderCalls(), [300], 'scrolling near the top starts only one real request');
    const anchored = await recentRows('before-prepend');
    await settlePage({ error: 'history fixture unavailable' });
    await page.waitForTimeout(250);
    await page.mouse.wheel(0, -120);
    await page.waitForTimeout(150);
    assert.deepEqual(await olderCalls(), [300], 'a failed page waits for an explicit retry');
    await page.getByRole('button', { name: '加载更早的历史', exact: true }).click();
    await waitForPage();
    const loadingEarlier = page.getByRole('button', { name: '正在加载更早的历史…', exact: true });
    assert.equal(await loadingEarlier.getAttribute('aria-disabled'), 'true');
    await loadingEarlier.click({ force: true });
    await page.mouse.move(700, 540);
    assert.deepEqual(await olderCalls(), [300, 300], 'retry uses the previous cursor');
    await settlePage({ events: [{ type: 'fixture/metadata', seq: 299, data: {} }], hasMore: true });
    await page.waitForTimeout(250);
    await page.getByRole('button', { name: '加载更早的历史', exact: true }).waitFor();
    assert.ok((await recentRows('metadata-only-page')).equals(anchored), 'metadata-only paging holds the current rows in place');

    await page.mouse.wheel(0, -120);
    await waitForPage();
    assert.deepEqual(await olderCalls(), [300, 300, 299], 'a page with no display rows still advances the cursor');
    await page.evaluate(({ sessionId, event }) => window.__verification.emit({
      kind: 'session_update', sessionId, update: { sessionUpdate: 'gpui_trajectory_event', event },
    }), { sessionId, event: paginatedToolCall(600, 'live') });
    await settlePage({ events: Array.from({ length: 70 }, (_, index) => paginatedToolCall(index + 220, 'older')), hasMore: false });
    await page.waitForTimeout(500);
    assert.ok((await recentRows('after-prepend')).equals(anchored), 'prepend holds the same records at the same visible positions');
    await page.mouse.wheel(0, -9000);
    await page.waitForTimeout(300);
    assert.ok(!(await recentRows('older-history-reachable')).equals(anchored), 'the newly loaded rows are reachable');
    assert.deepEqual(await olderCalls(), [300, 300, 299], 'the end of available history disarms automatic loading');
  } else if (mode === 'trajectory-timeline') {
    await clickConversationTab(1318);
    const timeline = page.getByRole('region', { name: '轨迹时间线', exact: true });
    await timeline.waitFor();
    const track = page.getByLabel('时间线概览；水平拖动可聚焦事件', { exact: true });
    const bounds = await track.boundingBox();
    assert.equal(await page.locator('[data-timeline-span]').count(), 31, 'the timeline retains all unfiltered operations');
    await page.mouse.move(800, 420); await page.mouse.wheel(0, -8000); await page.waitForTimeout(300);
    const ledger = async name => {
      const shot = await page.screenshot({ path: join(artifacts, `${name}.png`) });
      return page.evaluate(source => new Promise(resolve => {
        const image = new Image(); image.onload = () => {
          const canvas = document.createElement('canvas'); canvas.width = image.width; canvas.height = image.height;
          const context = canvas.getContext('2d'); context.drawImage(image, 0, 0);
          const { data } = context.getImageData(1420, 133, 18, 800);
          const groups = [];
          for (let y = 0; y < 800; y++) {
            let matches = 0;
            for (let x = 0; x < 18; x++) {
              const [red, green, blue] = data.slice((y * 18 + x) * 4, (y * 18 + x) * 4 + 3);
              if (blue - red > 8 && green - red > 3) matches++;
            }
            if (matches >= 6) {
              if (!groups.length || y - groups.at(-1).at(-1) > 3) groups.push([y]); else groups.at(-1).push(y);
            }
          }
          resolve(groups.map(group => group[Math.floor(group.length / 2)]));
        }; image.src = `data:image/png;base64,${source}`;
      }), shot.toString('base64'));
    };
    const before = await ledger('timeline-before-selection');
    const x = fraction => bounds.x + bounds.width * fraction;
    await page.mouse.move(x(1.1 / 31), bounds.y + 4);
    await page.mouse.down(); await page.mouse.move(x(5.9 / 31), bounds.y + 4, { steps: 12 }); await page.mouse.up();
    await page.waitForFunction(() => document.querySelectorAll('[data-selected="true"][data-timeline-span]').length === 5);
    assert.equal(await page.locator('[data-selected="false"][data-timeline-span]').count(), 26);
    const focused = await ledger('timeline-focused-selection');
    assert.ok(before.length > 15 && focused.length === before.length, 'focusing preserves the visible ledger rows');
    assert.deepEqual(focused, before, 'outside rows are faded in place instead of removed');
    await track.focus(); await page.keyboard.press('Escape');
    await page.waitForFunction(() => !document.querySelector('[data-selected][data-timeline-span]'));
    const firstTool = page.locator('[data-timeline-span="tool"]').first();
    await firstTool.hover();
    await page.getByRole('tooltip').waitFor();
    assert.match(await page.getByRole('tooltip').innerText(), /工具/);
    await firstTool.click();
    await page.getByRole('separator', { name: '调整事件详情宽度' }).waitFor();
    assert.equal(await firstTool.getAttribute('data-current'), 'true');
    await page.keyboard.press('Escape');
    await page.getByRole('separator', { name: '调整事件详情宽度' }).waitFor({ state: 'detached' });
    const domain = page.locator('[data-timeline-domain]');
    await page.mouse.move(x(0.5), bounds.y + 20); await page.mouse.wheel(0, -800);
    await page.waitForTimeout(250);
    const projection = () => domain.evaluate(el => [parseFloat(el.style.getPropertyValue('--trajectory-domain-left')), parseFloat(el.style.getPropertyValue('--trajectory-domain-width'))]);
    const zoomed = await projection(); assert.ok(zoomed[1] > 100 && zoomed[1] <= 775);
    await page.mouse.down({ button: 'right' }); await page.mouse.move(x(0.65), bounds.y + 20, { steps: 8 }); await page.mouse.up({ button: 'right' });
    const panned = await projection(); assert.notEqual(panned[0], zoomed[0]); assert.ok(Math.abs(panned[1] - zoomed[1]) < 0.00001);
    await page.mouse.wheel(0, 10000); await page.waitForTimeout(250);
    assert.deepEqual(await projection(), [0, 100], 'zooming out restores the full domain');
    await page.screenshot({ path: join(artifacts, 'timeline-reset.png') });
  } else if (mode === 'trajectory-menus') {
    await clickConversationTab(1318);
    const timeline = page.getByRole('region', { name: '轨迹时间线', exact: true });
    await timeline.waitFor();
    const bounds = await timeline.boundingBox();
    // The taller session menu intersects both the toolbar and the timeline.
    const moreX = page.viewportSize().width - 58;
    await page.mouse.click(moreX, 25);
    await page.waitForFunction(() => !!document.querySelector('.trajectory-timeline-frame')?.style.clipPath);
    await page.waitForTimeout(250);
    await page.screenshot({ path: join(artifacts, 'session-menu-over-timeline.png') });
    const region = { x: moreX - 104, y: Math.ceil(bounds.y + 4), width: 108, height: 34 };
    const sample = () => page.screenshot({ clip: region });
    const painted = await sample();
    const ghost = await page.addStyleTag({ content: '.trajectory-timeline-frame { visibility: hidden !important }' });
    try {
      assert.ok(painted.equals(await sample()), 'the timeline paints no pixels through the session menu');
    } finally { await ghost.evaluate(node => node.remove()); }
    await page.mouse.click(moreX, 25);
    await page.waitForFunction(() => !document.querySelector('.trajectory-timeline-frame')?.style.clipPath);
  } else if (mode === 'trajectory-folds') {
    await clickConversationTab(1318);
    const timeline = page.getByRole('region', { name: '轨迹时间线', exact: true });
    await timeline.waitFor();
    await page.waitForTimeout(300);
    const bounds = await timeline.boundingBox();
    const top = Math.round(bounds.y + bounds.height) - 1;
    const toolbar = page.getByRole('toolbar', { name: '轨迹工具栏', exact: true });
    const duration = toolbar.getByRole('button', { name: '使用实际时长', exact: true });
    await duration.hover();
    const durationIdle = await duration.evaluate(node => getComputedStyle(node).backgroundColor);
    await duration.click();
    await page.waitForFunction(() => document.querySelector('[aria-label="使用实际时长"]')?.getAttribute('aria-pressed') === 'true');
    assert.notEqual(await duration.evaluate(node => getComputedStyle(node).backgroundColor), durationIdle,
      'the selected fill changes even while the pointer remains over the button');
    await duration.click();
    // Canvas menus must cover only their own rectangle, without unmounting the
    // React toolbar/timeline or intercepting controls outside that rectangle.
    await page.mouse.click(page.viewportSize().width - 93, 25);
    await page.waitForFunction(() => !!document.querySelector('.trajectory-timeline-frame')?.style.clipPath,
      null, { timeout: 5000 });
    assert.equal(await toolbar.isVisible(), true, 'opening an application menu preserves the toolbar');
    assert.equal(await timeline.isVisible(), true, 'opening an application menu preserves the timeline');
    assert.equal(await page.evaluate(() => {
      const toolbar = document.querySelector('[aria-label="轨迹工具栏"]');
      const rect = toolbar.getBoundingClientRect();
      return !!document.elementFromPoint(rect.right - 100, rect.top + 15)?.closest('.trajectory-timeline-frame');
    }), false, 'the canvas menu owns pointer hits inside its painted bounds');
    await page.screenshot({ path: join(artifacts, 'application-menu-preserves-timeline.png') });
    await page.mouse.click(page.viewportSize().width - 93, 25);
    await page.waitForFunction(() => !document.querySelector('.trajectory-timeline-frame')?.style.clipPath,
      null, { timeout: 5000 });
    const ledger = async name => {
      await page.mouse.move(900, 26);
      await page.waitForTimeout(250);
      const shot = await page.screenshot({ path: join(artifacts, `${name}.png`) });
      return page.evaluate(({ source, top }) => new Promise((resolve, reject) => {
        const image = new Image();
        image.onload = () => {
          const canvas = document.createElement('canvas');
          canvas.width = image.width; canvas.height = image.height;
          const ctx = canvas.getContext('2d'); ctx.drawImage(image, 0, 0);
          const data = ctx.getImageData(1100, top, 1, 300).data;
          const edges = [];
          for (let y = 1; y < 298; y++) {
            const delta = Math.max(...[0, 1, 2].map(c => Math.abs(data[y * 4 + c] - data[(y + 1) * 4 + c])));
            if (delta >= 8 && (edges.at(-1) ?? -10) < y - 1) edges.push(y);
          }
          resolve(edges);
        };
        image.onerror = reject; image.src = source;
      }), { source: `data:image/png;base64,${shot.toString('base64')}`, top });
    };
    assert.deepEqual(await ledger('expanded'), [29, 59, 89, 119, 149, 179, 209]);
    await toolbar.getByRole('button', { name: '收起所有子调用', exact: true }).click();
    assert.deepEqual(await ledger('subcalls-folded'), [29, 59, 89, 109, 139, 169], 'Subcalls preserves root and sibling calls');
    assert.notEqual(await toolbar.getByRole('button', { name: '展开所有子调用', exact: true })
      .evaluate(node => getComputedStyle(node).backgroundColor), 'rgba(0, 0, 0, 0)', 'fold selection remains filled after pointer leave');
    await toolbar.getByRole('button', { name: '收起所有调用', exact: true }).click();
    assert.deepEqual(await ledger('calls-folded'), [29, 59, 79, 109], 'Calls preserves the assistant and following message');
    await toolbar.getByRole('button', { name: '收起所有轮次', exact: true }).click();
    assert.deepEqual(await ledger('turn-folded'), [29, 49]);
    const search = toolbar.getByRole('searchbox', { name: '搜索轨迹', exact: true });
    await search.fill('needle');
    await page.waitForFunction(() => document.querySelectorAll('[data-search-match="true"]').length === 1);
    assert.deepEqual(await ledger('search-through-all-folds'), [29], 'search exposes only the matching nested call');
    await search.fill('');
    await page.waitForFunction(() => document.querySelectorAll('[data-search-match]').length === 0);
    assert.deepEqual(await ledger('cleared-search'), [29, 49]);
    await toolbar.getByRole('button', { name: '展开所有轮次', exact: true }).click();
    assert.deepEqual(await ledger('turn-expanded'), [29, 59, 79, 109]);
    await toolbar.getByRole('button', { name: '展开所有调用', exact: true }).click();
    assert.deepEqual(await ledger('calls-expanded'), [29, 59, 89, 109, 139, 169]);
    await toolbar.getByRole('button', { name: '展开所有子调用', exact: true }).click();
    assert.deepEqual(await ledger('all-expanded'), [29, 59, 89, 119, 149, 179, 209]);
    for (const key of ['Enter', 'Space']) {
      // Use separate points on the same row so GPUI counts two double clicks
      // even when the keyboard assertions finish within its 400 ms click window.
      await page.mouse.dblclick(key === 'Enter' ? 600 : 620, top + 15);
      await toolbar.getByRole('button', { name: '展开所有轮次', exact: true }).waitFor();
      await page.keyboard.press('Tab');
      if (key === 'Space') {
        await page.keyboard.press('Shift+Tab');
        await page.keyboard.press('Tab');
      }
      await page.screenshot({ path: join(artifacts, `summary-before-${key}.png`) });
      await writeFile(join(artifacts, `focus-before-${key}.json`), JSON.stringify(await page.evaluate(() => ({ tag: document.activeElement?.tagName, id: document.activeElement?.id, label: document.activeElement?.getAttribute('aria-label') }))));
      await page.keyboard.press(key);
      await toolbar.getByRole('button', { name: '收起所有轮次', exact: true }).waitFor({ timeout: 5000 });
    }
    assert.equal(errors.length, 0);
  } else if (mode === 'trajectory-long') {
    await clickConversationTab(1318);
    await page.waitForTimeout(900);
    const timelineBounds = await page.getByRole('region', { name: '轨迹时间线', exact: true }).boundingBox();
    const trajectoryOffset = Math.round(timelineBounds.y + timelineBounds.height) - 134;
    // Read the rendered blue status dots. The Canvas has no DOM or native
    // accessibility nodes; no private Rust layout mirror is used here.
    const rows = async name => {
      const screenshot = await page.screenshot({ path: join(artifacts, `${name}.png`) });
      const png = await page.evaluate(({ source, trajectoryOffset }) => new Promise((resolve, reject) => {
        const image = new Image();
        image.onload = () => {
          const canvas = document.createElement('canvas');
          canvas.width = image.width;
          canvas.height = image.height;
          const context = canvas.getContext('2d');
          context.drawImage(image, 0, 0);
          const { data } = context.getImageData(1418, 132 + trajectoryOffset, 22, 846 - trajectoryOffset);
          const centers = [];
          for (let y = 1; y < 845 - trajectoryOffset; y++) {
            let matches = 0;
            for (let x = 1; x < 21; x++) {
              const offset = (y * 22 + x) * 4;
              const [red, green, blue, alpha] = data.slice(offset, offset + 4);
              if (alpha > 250 && blue * 255 / alpha > 180 && (blue - red) * 255 / alpha > 50 && (green - red) * 255 / alpha > 25) matches++;
            }
            if (matches >= 10) centers.push(y + 132 + trajectoryOffset);
          }
          const groups = [];
          for (const y of centers) {
            if (!groups.length || y - groups.at(-1).at(-1) > 3) groups.push([y]);
            else groups.at(-1).push(y);
          }
          const tracked = groups.map(group => Math.round((group[0] + group.at(-1)) / 2));
          // Ignore row centers below the composer's top edge; those rows are
          // below the scroll viewport even though their sprites are painted.
          resolve({ positions: tracked.filter(y => y < 958) });
        };
        image.onerror = reject;
        image.src = source;
      }), { source: `data:image/png;base64,${screenshot.toString('base64')}`, trajectoryOffset });
      assert.ok(png.positions.length >= 2, `${name} contains recognizable trajectory records`);
      return png;
    };
    const initial = (await rows('initial-tail')).positions;
    assert.ok(initial.at(-1) > 890 && initial.at(-1) < 940, `initial history follows its tail: ${initial}`);
    await page.mouse.move(700, 500);
    await page.mouse.wheel(0, -450);
    await page.waitForTimeout(500);
    const upward = (await rows('upward')).positions;
    assert.ok(upward[0] < 290, `upward scrolling returns to the earliest tool record: ${upward}`);
    await page.evaluate(sessionId => window.__verification.emit({
      kind: 'session_update', sessionId,
      update: { sessionUpdate: 'gpui_trajectory_event', event: { type: 'tool/call', seq: 100, time: 8000, data: { turn: 1, step: 1, callId: 'new-record', name: 'bash', arguments: '{"command":"new-record"}' } } },
    }), sessionId);
    await page.waitForTimeout(400);
    const stationary = (await rows('stationary')).positions;
    assert.deepEqual(stationary, upward, 'new records do not move the visible reading position');
    for (let index = 0; index < 5; index++) {
      await page.mouse.wheel(0, 120);
      await page.waitForTimeout(100);
    }
    await page.waitForTimeout(500);
    const bottom = await rows('bottom');
    assert.ok(Math.abs(bottom.positions.at(-1) - initial.at(-1)) <= 2, 'bottom proximity restores the tail');
    assert.ok(bottom.positions.at(-1) < 940, `the final record clears the floating composer: ${bottom.positions}`);
    await page.mouse.wheel(0, 500);
    await page.waitForTimeout(300);
    const anchor = (await rows('bottom-anchor')).positions;
    assert.equal(anchor.length, bottom.positions.length, 'the extra wheel gesture stays at the tail');
    await page.mouse.wheel(0, -120);
    await page.waitForTimeout(300);
    const interrupted = (await rows('interrupted')).positions;
    const anchorVisible = anchor.filter(y => y >= 172 && y < 952 - 120);
    const interruptedVisible = interrupted.filter(y => y >= 172 + 120 && y < 952);
    assert.ok(anchorVisible.length > 0, 'the shared visible interval contains records');
    assert.deepEqual(
      interruptedVisible,
      anchorVisible.map(y => y + 120),
      'upward input moves the same visible records toward older records'
    );
    await page.mouse.wheel(0, 500);
    await page.waitForTimeout(300);
    assert.ok(Math.abs((await rows('bottom-restored')).positions.at(-1) - bottom.positions.at(-1)) <= 2, 'the next bottom gesture restores following');
    await page.evaluate(sessionId => window.__verification.emit({
      kind: 'session_update', sessionId,
      update: { sessionUpdate: 'gpui_trajectory_event', event: { type: 'tool/call', seq: 101, time: 8200, data: { turn: 1, step: 1, callId: 'follow-record', name: 'bash', arguments: '{"command":"follow-record"}' } } },
    }), sessionId);
    await page.waitForTimeout(500);
    const followed = (await rows('followed-tail')).positions;
    assert.ok(Math.abs(followed.at(-1) - bottom.positions.at(-1)) <= 2, 'new record follows the tail after bottom re-entry');
    const clearance = await page.evaluate(({ lastDot, source }) => new Promise((resolve, reject) => {
      const image = new Image();
      image.onload = () => {
        const canvas = document.createElement('canvas');
        canvas.width = image.width;
        canvas.height = image.height;
        const context = canvas.getContext('2d');
        context.drawImage(image, 0, 0);
        const rowBottom = lastDot + 15;
        const gap = [];
        for (let y = rowBottom + 1; y <= rowBottom + 16; y++) {
          const [red, green, blue] = context.getImageData(1000, y, 1, 1).data;
          gap.push(red < 40 && green < 40 && blue < 40);
        }
        let composerTop = null;
        for (let y = rowBottom + 17; y < 1050; y++) {
          const [red, green, blue] = context.getImageData(860, y, 1, 1).data;
          if (Math.max(red, green, blue) > 48) { composerTop = y; break; }
        }
        resolve({ rowBottom, gap, composerTop });
      };
      image.onerror = reject;
      image.src = source;
    }), { lastDot: followed.at(-1), source: `data:image/png;base64,${(await readFile(join(artifacts, 'followed-tail.png'))).toString('base64')}` });
    assert.ok(clearance.gap.every(Boolean), `16px below the final record is empty: ${JSON.stringify(clearance)}`);
    assert.ok(clearance.composerTop - clearance.rowBottom >= 16, `composer leaves the measured record clearance: ${JSON.stringify(clearance)}`);
    assert.equal(errors.length, 0, `no browser errors: ${errors.join('\n')}`);
    await writeFile(join(artifacts, 'report.json'), JSON.stringify({ initial, upward, stationary, bottom, interrupted, followed, errors }, null, 2));
  } else if (mode === 'turn-navigation') {
    const countTicks = async name => {
      const screenshot = await page.screenshot({ path: join(artifacts, `${name}.png`) });
      return page.evaluate(async data => {
        const image = new Image();
        image.src = data;
        await image.decode();
        const canvas = document.createElement('canvas');
        canvas.width = 28;
        canvas.height = image.height;
        const context = canvas.getContext('2d');
        context.drawImage(image, image.width - 40, 0, 28, image.height, 0, 0, 28, image.height);
        const { data: pixels } = context.getImageData(0, 0, 28, image.height);
        let hits = 0;
        let previous = false;
        // A tick occupies the right part of the rail, with plain background to its left.
        for (let y = 200; y < image.height - 235; y++) {
          const offset = y * 28 * 4;
          const tick = pixels[offset + 24 * 4] - pixels[offset + 2 * 4] >= 16
            && pixels[offset + 22 * 4] - pixels[offset + 2 * 4] >= 16;
          if (tick && !previous) hits++;
          previous = tick;
        }
        return hits;
      }, `data:image/png;base64,${screenshot.toString('base64')}`);
    };
    await page.waitForTimeout(500);
    assert.equal(await countTicks('chat-wide'), 2);
    await clickConversationTab(1318);
    await page.waitForTimeout(500);
    assert.equal(await countTicks('trajectory-wide'), 0);
    await clickConversationTab(1271);
    await page.waitForTimeout(500);
    assert.equal(await countTicks('chat-restored'), 2);
    await page.setViewportSize({ width: 964, height: 822 });
    await page.waitForTimeout(500);
    assert.equal(await countTicks('chat-narrow'), 0);
  } else if (mode === 'trajectory-prompts') {
    await page.waitForTimeout(500);
    await clickConversationTab(1318);
    await page.waitForTimeout(600);
    const timelineBounds = await page.getByRole('region', { name: '轨迹时间线', exact: true }).boundingBox();
    const trajectoryOffset = Math.round(timelineBounds.y + timelineBounds.height) - 134;
    const click = async (x, y) => { y += trajectoryOffset; await page.mouse.move(x, y); await page.waitForTimeout(80); await page.mouse.click(x, y, { delay: 60 }); await page.waitForTimeout(250); };
    await page.screenshot({ path: join(artifacts, 'prompt-ledger.png') });
    await click(600, 205);
    const frame = page.locator('.trajectory-json-frame:not([hidden])');
    await frame.locator('pre').waitFor({ state: 'visible' });
    assert.ok((await frame.textContent()).includes('-保留原始内容。'));
    assert.ok((await frame.textContent()).includes('+保留完整内容。'));
    assert.deepEqual(await frame.locator('h3').allTextContents(), ['系统提示词']);
    assert.equal(await frame.locator('pre').evaluate(node => getComputedStyle(node).fontSize), '11px');
    assert.equal(await frame.locator('pre').evaluate(node => getComputedStyle(node).lineHeight), '17px');
    await page.screenshot({ path: join(artifacts, 'system-diff.png') });
    await click(600, 235);
    await frame.locator('pre').waitFor({ state: 'visible' });
    await page.waitForFunction(() => [...document.querySelectorAll('.trajectory-json-frame:not([hidden]) h3')].map(node => node.textContent).join('|') === '工具');
    assert.deepEqual(await frame.locator('h3').allTextContents(), ['工具']);
    assert.ok((await frame.textContent()).includes('read_file'));
    await page.screenshot({ path: join(artifacts, 'tools-diff.png') });
    await click(1176, 197);
    await frame.locator('summary').first().waitFor({ state: 'visible' });
    assert.equal(await frame.locator('summary').count(), 2);
    assert.equal(await frame.locator('details[open]').count(), 0);
    const summary = frame.locator('summary').nth(1);
    await summary.focus(); await page.keyboard.press('Enter');
    const tree = page.getByRole('tree', { name: 'read_file 参数 JSON' });
    await tree.waitFor({ state: 'visible' });
    assert.ok((await frame.textContent()).includes('保留原始换行。'));
    assert.equal(await summary.evaluate(node => getComputedStyle(node).minHeight), '30px');
    await page.screenshot({ path: join(artifacts, 'tool-catalog.png') });
    await page.keyboard.press('Enter');
    assert.equal(await frame.locator('details[open]').count(), 0);
    await click(600, 175);
    await page.waitForFunction(() => document.querySelectorAll('.trajectory-json-frame:not([hidden]) summary').length === 1, undefined, { timeout: 5000 });
    await page.screenshot({ path: join(artifacts, 'initial-tools.png') });
    assert.equal(await frame.locator('summary').count(), 1, 'initial snapshot retains its original tool catalog');
    await click(1040, 197);
    await page.waitForFunction(() => !document.querySelector('.trajectory-json-frame:not([hidden])'));
    await page.screenshot({ path: join(artifacts, 'initial-system-markdown.png') });
    await click(600, 265);
    await click(1020, 197);
    await frame.locator('pre').first().waitFor({ state: 'visible' });
    await page.waitForFunction(() => [...document.querySelectorAll('.trajectory-json-frame:not([hidden]) h3')].map(node => node.textContent).join('|') === '系统提示词|工具');
    assert.deepEqual(await frame.locator('h3').allTextContents(), ['系统提示词', '工具']);
    await page.screenshot({ path: join(artifacts, 'combined-diff.png') });
    await click(1176, 197);
    await frame.getByText('本次请求没有工具', { exact: true }).waitFor({ state: 'visible' });
    await page.screenshot({ path: join(artifacts, 'empty-tools.png') });
    await click(600, 295);
    await click(1098, 197);
    await page.waitForFunction(() => !document.querySelector('.trajectory-json-frame:not([hidden])'));
    await page.screenshot({ path: join(artifacts, 'empty-system.png') });
  } else if (mode === 'trajectory-program') {
    await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
    await page.getByRole('tab', { name: '轨迹', exact: true }).evaluate(node => node.click());
    const controls = page.locator('[data-trajectory-accessibility="controls"]');
    const tabs = page.getByRole('tablist', { name: '事件详情', exact: true });
    const source = page.locator('.trajectory-program-full');
    const select = async name => {
      const target = controls.getByRole('button', { name });
      await target.waitFor(); await target.evaluate(node => node.click());
    };
    await select(/^#\d+ 工具 run_code Inspect structured source/);
    await tabs.getByRole('tab', { name: '源码', exact: true }).evaluate(node => node.click());
    await source.locator('pre.shiki').waitFor();
    assert.equal(await source.locator('pre').textContent(), programSource.slice(0, -1));
    assert.equal(await source.locator('[data-line-numbers="true"]').count(), 1);
    assert.equal(await source.locator('[data-code-block-banner]').count(), 0);
    assert.equal(await source.getByText('typescript', { exact: true }).count(), 1);
    const pre = source.locator('pre');
    assert.deepEqual(await pre.evaluate(node => ({ font: getComputedStyle(node).fontSize, line: getComputedStyle(node).lineHeight,
      tab: getComputedStyle(node).tabSize })), { font: '12px', line: '16px', tab: '2' });
    assert.equal(await pre.locator('b').count(), 0, 'Source markup is rendered as literal text');
    await source.getByRole('button', { name: '复制源码', exact: true }).click();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), programSource, 'Copy retains the original trailing newline');
    const wrap = source.getByRole('button', { name: '自动换行', exact: true });
    assert.equal(await wrap.getAttribute('aria-pressed'), 'false');
    await wrap.click();
    assert.equal(await wrap.getAttribute('aria-pressed'), 'true');
    assert.equal(await page.evaluate(() => localStorage.getItem('dsh.trajectory.jsonStringWrapping')), 'true');
    await page.screenshot({ path: join(artifacts, 'source-wrapped.png') });
    const sourceScroller = source.locator('[data-wrap]');
    const sourceHeadingY = (await source.locator('header').boundingBox()).y;
    const sourceBox = await sourceScroller.boundingBox();
    await page.mouse.move(sourceBox.x + sourceBox.width / 2, sourceBox.y + Math.min(sourceBox.height / 2, 180));
    await page.mouse.wheel(0, 650);
    await page.waitForFunction(() => document.querySelector('.trajectory-program-full [data-wrap]')?.scrollTop > 100);
    assert.equal((await source.locator('header').boundingBox()).y, sourceHeadingY, 'the source toolbar stays fixed while code scrolls');
    await page.screenshot({ path: join(artifacts, 'source-scrolled.png') });
    await page.mouse.wheel(0, 100000);
    await page.waitForFunction(() => {
      const node = document.querySelector('.trajectory-program-full [data-wrap]');
      return node && node.scrollTop + node.clientHeight >= node.scrollHeight - 1;
    });
    await page.mouse.wheel(0, 650);
    await page.waitForTimeout(150);
    assert.equal((await source.locator('header').boundingBox()).y, sourceHeadingY, 'the source boundary must not scroll the outer inspector');
    await page.mouse.wheel(0, -100000);
    await page.waitForFunction(() => document.querySelector('.trajectory-program-full [data-wrap]')?.scrollTop === 0);
    await page.mouse.wheel(0, -650);
    await page.waitForTimeout(150);
    assert.equal((await source.locator('header').boundingBox()).y, sourceHeadingY, 'the first source line also contains overscroll');
    await source.getByRole('button', { name: '原始 JSON', exact: true }).click();
    await source.getByRole('tree', { name: '参数 JSON', exact: true }).waitFor();
    assert.equal(await source.getByRole('button', { name: '自动换行', exact: true }).count(), 0);
    await source.getByRole('button', { name: '复制 JSON', exact: true }).click();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), programArguments, 'Copy preserves recorded JSON whitespace');
    await tabs.getByRole('tab', { name: '结果', exact: true }).evaluate(node => node.click());
    await page.getByRole('tree', { name: '输出 JSON', exact: true }).waitFor();
    await page.screenshot({ path: join(artifacts, 'rendered-output.png') });
    await tabs.getByRole('tab', { name: '概述', exact: true }).evaluate(node => node.click());
    await page.getByRole('button', { name: '源码', exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '原始 JSON', exact: true }).count(), 0, 'Overview only previews source');
    await page.getByRole('button', { name: '源码', exact: true }).click();
    await source.locator('pre.shiki').waitFor();
    assert.equal(await source.getByRole('button', { name: '自动换行', exact: true }).getAttribute('aria-pressed'), 'true');
    await select(/^#\d+ 工具 cua_run_code Python failure/);
    await tabs.getByRole('tab', { name: '源码', exact: true }).evaluate(node => node.click());
    await source.getByText('python', { exact: true }).waitFor();
    await tabs.getByRole('tab', { name: '结果', exact: true }).evaluate(node => node.click());
    const failure = page.locator('.trajectory-json-frame pre').filter({ hasText: 'Error: example failure' });
    await failure.waitFor();
    assert.equal(await failure.textContent(), 'Error: example failure\nCaptured output:\n<b>literal</b>');
    assert.equal(await failure.locator('b').count(), 0);
    assert.equal(await failure.evaluate(node => getComputedStyle(node, '::first-line').fontWeight), '600');
    await select(/^#\d+ 工具 run_code.*description.*null/);
    await tabs.getByRole('tab', { name: '参数', exact: true }).waitFor();
    assert.equal(await tabs.getByRole('tab', { name: '源码', exact: true }).count(), 0);
    await select(/^#\d+ 工具 run_code Still running/);
    await tabs.getByRole('tab', { name: '结果', exact: true }).evaluate(node => node.click());
    await page.locator('.trajectory-json-frame').getByText('正在运行…', { exact: true }).waitFor();
    await page.screenshot({ path: join(artifacts, 'running-program.png') });
    assert.equal(errors.length, 0, `no browser errors: ${errors.join('\n')}`);
  } else if (mode === 'trajectory-blocks') {
    await runOutputBlocksCase(page, artifacts);
  } else if (mode === 'trajectory-results') {
    await page.getByRole('tab', { name: '轨迹', exact: true }).evaluate(node => node.click());
    const controls = page.locator('[data-trajectory-accessibility="controls"]');
    const first = controls.getByRole('button', { name: /^#\d+ 工具 bash/ });
    const second = controls.getByRole('button', { name: /^#\d+ 工具 read_file/ });
    const tabs = page.getByRole('tablist', { name: '事件详情', exact: true });
    const tree = page.locator('[data-trajectory-kind="raw-result"]').getByRole('tree', { name: 'JSON 内容', exact: true });
    const scalar = page.locator('.trajectory-result-scalar');
    const watch = () => page.evaluate(() => window.__verification.toolResultWatch);
    const selectResult = async record => {
      await record.waitFor(); await record.evaluate(node => node.click());
      await tabs.getByRole('tab', { name: '结果', exact: true }).evaluate(node => node.click());
    };
    const publish = (owner, update, available, value) => page.evaluate(({ owner, update, available, value }) => {
      window.__verification.emit({ kind: 'tool_result', ...owner, update, available, value });
    }, { owner, update, available, value });
    await selectResult(first);
    await page.waitForFunction(() => window.__verification.toolResultWatch?.seq === 6);
    const original = await watch();
    await page.waitForFunction(() => !document.querySelector('[data-trajectory-kind^="raw-result"]'));
    await page.locator('.trajectory-tool-text-output pre').getByText('/tmp', { exact: true }).waitFor();
    await publish(original, 2, true, { rawOnly: 'not the rendered /tmp', nested: [false, null, 7] });
    await tree.getByText('rawOnly:', { exact: true }).waitFor();
    await page.screenshot({ path: join(artifacts, 'live-raw-object.png') });
    await publish(original, 1, false);
    assert.ok(await tree.getByText('rawOnly:', { exact: true }).isVisible(), 'late stream frames cannot replace a newer value');
    for (const [index, value] of [null, false, 0, 'literal <b> &\n'].entries()) {
      await publish(original, index + 3, true, value);
      await page.waitForFunction(expected => document.querySelector('.trajectory-result-scalar')?.textContent === expected, JSON.stringify(value));
      assert.equal(await scalar.locator('*').count(), 0, 'scalar values are escaped text, not interpreted markup');
    }
    assert.deepEqual(await scalar.evaluate(node => ({ font: getComputedStyle(node).fontSize, line: getComputedStyle(node).lineHeight,
      wrap: getComputedStyle(node).whiteSpace })), { font: '12px', line: '16px', wrap: 'pre-wrap' });
    await publish(original, 7, false);
    await scalar.waitFor({ state: 'detached' });
    await selectResult(second);
    await page.waitForFunction(() => window.__verification.toolResultWatch?.seq === 10);
    const other = await watch();
    const rendered = page.locator('.trajectory-json-frame:not([hidden]) .trajectory-tool-text-output pre');
    await rendered.filter({ hasText: 'Other rendered result' }).waitFor();
    await page.waitForFunction(() => document.querySelectorAll('.trajectory-json-frame:not([hidden]) .trajectory-tool-text-output pre').length === 1);
    assert.equal(await rendered.textContent(), 'Other rendered result\n<b>literal</b>\n```js\nconst x = 7;\n```\n');
    assert.equal(await rendered.locator('*').count(), 0, 'tool results preserve markup and code fences as literal text');
    assert.deepEqual(await rendered.evaluate(node => ({ font: getComputedStyle(node).fontSize, line: getComputedStyle(node).lineHeight,
      wrap: getComputedStyle(node).whiteSpace })), { font: '12px', line: '19px', wrap: 'pre-wrap' });
    assert.notEqual(original.token, other.token);
    await publish(original, 100, true, { staleOwner: true });
    await publish(other, 2, true, { currentOwner: 'second call' });
    await tree.getByText('currentOwner:', { exact: true }).waitFor();
    assert.equal(await tree.getByText('staleOwner:', { exact: true }).count(), 0);
    await page.getByRole('button', { name: '关闭详情', exact: true }).evaluate(node => node.click());
    await page.waitForFunction(() => window.__verification.toolResultWatch?.sessionId == null);
    await publish(other, 3, true, { closedOwner: true });
    await page.waitForFunction(() => !document.querySelector('[data-trajectory-kind^="raw-result"]'));
    await selectResult(second);
    await page.waitForFunction(token => window.__verification.toolResultWatch?.seq === 10 && window.__verification.toolResultWatch?.token !== token, other.token);
    const reopened = await watch();
    await page.waitForFunction(() => !document.querySelector('[data-trajectory-kind^="raw-result"]'));
    await publish(reopened, 2, true, { beforeDisconnect: true });
    await tree.getByText('beforeDisconnect:', { exact: true }).waitFor();
    await page.evaluate(() => window.__verification.emit({ kind: 'service', status: 'exited', detail: 'Fixture disconnect' }));
    await page.waitForFunction(() => !document.querySelector('[data-trajectory-kind^="raw-result"]'));
    await page.evaluate(() => window.__verification.emit({ kind: 'service', status: 'running' }));
    await page.waitForFunction(token => window.__verification.toolResultWatch?.seq === 10 && window.__verification.toolResultWatch?.token !== token, reopened.token);
    const reconnected = await watch();
    await publish(reopened, 99, true, { previousConnection: true });
    await publish(reconnected, 2, true, { reconnected: true });
    await tree.getByText('reconnected:', { exact: true }).waitFor();
    assert.equal(await tree.getByText('previousConnection:', { exact: true }).count(), 0);
    await page.screenshot({ path: join(artifacts, 'reconnected-result.png') });
    await writeFile(join(artifacts, 'calls.json'), JSON.stringify(await page.evaluate(() => window.__verification.calls.filter(call => call.name === 'watch_tool_result')), null, 2));
    assert.equal(errors.length, 0, `no browser errors: ${errors.join('\n')}`);
  } else if (mode === 'trajectory' || mode === 'trajectory-narrow') {
    await page.waitForTimeout(500);
    assert.equal((await page.evaluate(() => window.__verification.calls)).filter(call => ['new_session', 'prompt', 'queue_prompt'].includes(call.name)).length, 0, 'history remains read-only');
    if (mode === 'trajectory-narrow') {
      await page.setViewportSize({ width: 620, height: 820 });
      await page.waitForTimeout(500);
      await clickConversationTab(492);
    } else {
      await clickConversationTab(1318);
    }
    await page.waitForTimeout(700);
    const timeline = page.getByRole('region', { name: '轨迹时间线', exact: true });
    await timeline.waitFor();
    const timelineBounds = await timeline.boundingBox();
    await writeFile(join(artifacts, 'timeline-geometry.json'), JSON.stringify(await timeline.evaluate(el => ({ timeline: el.getBoundingClientRect().toJSON(), holder: el.parentElement.parentElement.getBoundingClientRect().toJSON(), content: el.parentElement.getBoundingClientRect().toJSON(), style: el.parentElement.getAttribute('style') })), null, 2));
    const toolbar = page.getByRole('toolbar', { name: '轨迹工具栏', exact: true });
    const toolbarBounds = await toolbar.boundingBox();
    assert.equal(toolbarBounds.y, 76, 'toolbar begins below the official two-row conversation header');
    assert.equal(toolbarBounds.x, timelineBounds.x);
    assert.equal(toolbarBounds.width, timelineBounds.width);
    const search = toolbar.getByRole('searchbox', { name: '搜索轨迹', exact: true });
    assert.equal(await search.getAttribute('placeholder'), '搜索');
    await search.fill('pwd');
    await page.waitForFunction(() => document.querySelectorAll('[data-search-match="true"]').length === 1);
    assert.equal(await page.locator('[data-search-match="true"]').getAttribute('data-timeline-span'), 'tool');
    assert.equal(await page.locator('[data-search-match="false"]').count(), 2, 'search dims unmatched timeline bands');
    await search.fill('no-such-trajectory-record');
    await page.waitForFunction(() => document.querySelectorAll('[data-search-match="false"]').length === 3);
    await search.fill('');
    await page.waitForFunction(() => document.querySelectorAll('[data-search-match]').length === 0);
    const trajectoryOffset = Math.round(timelineBounds.y + timelineBounds.height) - 134;
    const ledgerTop = Math.round(timelineBounds.y + timelineBounds.height) - 1 - trajectoryOffset;
    assert.ok(timelineBounds.y >= 108, 'timeline follows the 76px two-row header and official toolbar');
    const bars = page.locator('[data-timeline-span]');
    assert.equal(await bars.count(), 3, 'request boundaries do not create duplicate timeline operations');
    for (const bar of await bars.all()) {
      const bounds = await bar.boundingBox();
      assert.equal(bounds.height, 8);
      assert.ok(Math.abs(bounds.width - ((timelineBounds.width - 44) / 3 - 2)) < 1, 'default operations are equal-width bands');
    }
    const assistantBand = page.locator('[data-timeline-span="message"]');
    assert.equal(await assistantBand.getAttribute('data-assistant-timing'), 'true');
    assert.equal(await assistantBand.evaluate(el => el.style.getPropertyValue('--trajectory-assistant-ttft')), '37.5%');
    if (mode === 'trajectory') {
      await page.getByRole('button', { name: '使用实际时长', exact: true }).click();
      await page.waitForFunction(() => parseFloat(document.querySelector('[data-timeline-span="message"]').style.getPropertyValue('--trajectory-span-width')) > 60);
      const durationWidth = (await assistantBand.boundingBox()).width;
      assert.ok(Math.abs(durationWidth - ((timelineBounds.width - 44) * 2 / 3 - 2)) < 1, 'recorded duration compresses only idle time');
      await page.getByRole('button', { name: '使用实际时长', exact: true }).click();
      await page.waitForFunction(() => parseFloat(document.querySelector('[data-timeline-span="message"]').style.getPropertyValue('--trajectory-span-width')) < 34);
    }
    const layout = async name => {
      const image = await page.screenshot({ path: join(artifacts, `${name}.png`) });
      let offset = 8;
      const chunks = [];
      let width;
      let height;
      while (offset < image.length) {
        const length = image.readUInt32BE(offset);
        const type = image.toString('ascii', offset + 4, offset + 8);
        if (type === 'IHDR') {
          width = image.readUInt32BE(offset + 8);
          height = image.readUInt32BE(offset + 12);
          assert.equal(image[offset + 16], 8, 'screenshot uses eight-bit color');
          assert.equal(image[offset + 17], 2, 'screenshot uses RGB color');
        }
        if (type === 'IDAT') chunks.push(image.subarray(offset + 8, offset + 8 + length));
        offset += length + 12;
      }
      const filtered = inflateSync(Buffer.concat(chunks));
      const stride = width * 3 + 1;
      const pixels = Buffer.alloc(filtered.length);
      const paeth = (left, above, upperLeft) => {
        const estimate = left + above - upperLeft;
        const dl = Math.abs(estimate - left), da = Math.abs(estimate - above), du = Math.abs(estimate - upperLeft);
        return dl <= da && dl <= du ? left : da <= du ? above : upperLeft;
      };
      for (let y = 0; y < height; y++) {
        const row = y * stride;
        const filter = filtered[row];
        assert.ok(filter <= 4, `supported PNG row filter ${filter}`);
        for (let x = 1; x < stride; x++) {
          const left = x > 3 ? pixels[row + x - 3] : 0;
          const above = y ? pixels[row - stride + x] : 0;
          const upperLeft = y && x > 3 ? pixels[row - stride + x - 3] : 0;
          let predictor = 0;
          if (filter === 1) predictor = left;
          else if (filter === 2) predictor = above;
          else if (filter === 3) predictor = (left + above) >> 1;
          else if (filter === 4) predictor = paeth(left, above, upperLeft);
          pixels[row + x] = (filtered[row + x] + predictor) & 255;
        }
      }
      // Canvas samples below use the fixture's previous content origin. Record
      // dimensions remain comparable while the actual header/toolbar can grow.
      const channel = (x, y, component) => pixels[(y + trajectoryOffset) * stride + 1 + x * 3 + component];
      const edges = [];
      const firstRecordY = 134;
      const rowSampleX = mode === 'trajectory-narrow' ? 598 : 800;
      const rowTransition = y => {
        const before = [0, 1, 2].map(component => channel(rowSampleX, y, component));
        const after = [0, 1, 2].map(component => channel(rowSampleX, y + 1, component));
        return Math.max(...before.map((value, index) => Math.abs(value - after[index])));
      };
      for (let y = firstRecordY; y < Math.min(height, 400); y++) {
        const delta = rowTransition(y);
        if (delta >= 8 && (edges.at(-1) ?? -10) < y - 1) edges.push(y);
      }
      const tagInk = [];
      const narrow = mode === 'trajectory-narrow';
      const tagBackground = [0, 1, 2].map(component => channel(335, 172, component));
      for (let y = 168; y < 185; y++) {
        for (let tagX = narrow ? 28 : 332; tagX < (narrow ? 47 : 361); tagX++) {
          const contrast = [0, 1, 2].map(component => Math.abs(channel(tagX, y, component) - tagBackground[component]));
          if (Math.max(...contrast) >= 50) tagInk.push([tagX, y]);
        }
      }
      const boundaryX = mode === 'trajectory-narrow' ? 6 : 292;
      const boundaryInk = [];
      for (let y = 125; y < Math.min(height, 400); y++) {
        for (let x = boundaryX + 7; x <= boundaryX + 9; x++) {
          const rgb = [0, 1, 2].map(component => channel(x, y, component));
          if (rgb.every(value => value >= 60 && value <= 170)) boundaryInk.push([x, y]);
        }
      }
      return { edges: edges.slice(0, 12), boundaryInk, tagInk, rowTransition, channel };
    };
    const expanded = await layout('trajectory');
    // The official result grid uses the table pane's container width, with a
    // 180..480px request column and an 8px gap before the arrow.
    const tableWidth = mode === 'trajectory-narrow' ? 620 : 1160;
    const contentLeft = mode === 'trajectory-narrow' ? 54 : 368;
    const arrowLeft = contentLeft + Math.min(480, Math.max(180, tableWidth * .36 - 56)) + 8;
    const arrowInk = [];
    for (let y = 196; y < 212; y++) {
      for (let x = Math.floor(arrowLeft - 3); x <= Math.ceil(arrowLeft + 12); x++) {
        if ([0, 1, 2].some(component => expanded.channel(x, y, component) >= 80)) arrowInk.push([x, y]);
      }
    }
    assert.ok(arrowInk.length >= 4, 'tool results occupy the separate official result column');
    assert.ok(Math.min(...arrowInk.map(([x]) => x)) >= Math.floor(arrowLeft), 'request/result gap preserves the official content padding');
    if (mode === 'trajectory-narrow') {
      assert.deepEqual(expanded.edges.slice(0, 3), [ledgerTop + 29, ledgerTop + 59, ledgerTop + 89], 'narrow history has three 30px content rows without a duplicate assistant row');
      assert.ok(expanded.tagInk.length > 0 && expanded.tagInk.every(([x]) => x >= 28 && x <= 46), `narrow type label leaves the 19px icon slot: ${JSON.stringify(expanded.tagInk)}`);
      assert.ok(expanded.boundaryInk.some(([, y]) => y >= 221 && y <= 231), `narrow terminal boundary dot stays inside its 9px seat: ${JSON.stringify(expanded.boundaryInk)}`);
      await page.mouse.click(292, 175 + trajectoryOffset);
      await page.waitForTimeout(292);
      const requestInspector = await layout('request-inspector');
      const inspectorTransition = layout => Math.max(...Array.from({ length: 120 }, (_, index) => index + 260).map(y =>
        Math.max(...[0, 1, 2].map(component => Math.abs(layout.channel(199, y, component) - layout.channel(200, y, component))))
      ));
      assert.ok(inspectorTransition(requestInspector) >= 8, 'narrow record opens the official right-side inspector drawer');
      await page.keyboard.press('Escape');
      await page.waitForTimeout(292);
      const closedInspector = await layout('request-inspector-closed');
      assert.ok(inspectorTransition(closedInspector) < 8, 'Escape closes the narrow inspector drawer');
      await page.mouse.click(14, 229 + trajectoryOffset);
      await page.waitForTimeout(292);
      const boundaryInspector = await layout('request-boundary-inspector');
      assert.ok(inspectorTransition(boundaryInspector) >= 8, 'the trailing request boundary opens its request details');
      await writeFile(join(artifacts, 'report.json'), JSON.stringify({ mode, ...expanded, errors }, null, 2));
    } else {
      assert.deepEqual(expanded.edges.slice(0, 3), [ledgerTop + 29, ledgerTop + 59, ledgerTop + 89], 'request metadata no longer duplicates assistant content');
      assert.ok(expanded.tagInk.length > 0 && expanded.tagInk.some(([x]) => x >= 351), `wide assistant kind tag shows its text label: ${JSON.stringify(expanded.tagInk)}`);
      assert.ok(expanded.boundaryInk.some(([, y]) => y >= 221 && y <= 231), `terminal boundary dot stays inside its 9px seat: ${JSON.stringify(expanded.boundaryInk)}`);
      await page.getByRole('searchbox', { name: '搜索轨迹', exact: true }).focus();
      await page.keyboard.press('Tab');
      await page.waitForFunction(() => document.activeElement?.getAttribute('aria-label') === '时间线概览；水平拖动可聚焦事件');
      await page.mouse.move(900, 26);
      const beforeRequest = await page.screenshot();
      const toolbarBefore = await page.getByRole('toolbar', { name: '轨迹工具栏', exact: true }).screenshot();
      await page.mouse.move(300, 229 + trajectoryOffset);
      await page.waitForTimeout(120);
      await page.mouse.click(300, 229 + trajectoryOffset, { delay: 70 });
      await page.waitForTimeout(292);
      const requestInspector = await page.screenshot({ path: join(artifacts, 'request-inspector.png') });
      assert.ok((await page.getByRole('toolbar', { name: '轨迹工具栏', exact: true }).screenshot()).equals(toolbarBefore), 'opening details preserves the full-width toolbar');
      const inspectorBounds = await page.getByRole('separator', { name: '调整事件详情宽度' }).boundingBox();
      assert.ok(inspectorBounds.y >= 130, 'the inspector begins below toolbar and timeline');
      assert.ok(inspectorBounds.y + inspectorBounds.height <= page.viewportSize().height - 80,
        'the inspector leaves room for the composer');
      assert.ok(!requestInspector.equals(beforeRequest), 'terminal request opens a real inspector');
      // The active tab's 2px blue underline is read from the rendered canvas.
      const activeDetailTab = shot => {
        const runs = [];
        for (let y = 204; y < 218; y++) {
          let start = null;
          for (let x = 994; x < 1431; x++) {
            const blue = shot.channel(x, y, 2), red = shot.channel(x, y, 0);
            const selected = blue > 150 && blue - red > 35;
            if (selected && start === null) start = x;
            if ((!selected || x === 1430) && start !== null) {
              if (x - start >= 20) runs.push({ center: (start + x - 1) / 2, length: x - start });
              start = null;
            }
          }
        }
        assert.ok(runs.length, 'inspector paints a selected detail tab');
        return runs.sort((a, b) => b.length - a.length)[0].center;
      };
      const detailClick = async (x, y) => {
        y += trajectoryOffset;
        await page.mouse.move(x, y);
        await page.waitForTimeout(120);
        await page.mouse.click(x, y, { delay: 70 });
        await page.waitForTimeout(220);
      };
      const requestOverview = activeDetailTab(await layout('request-overview'));
      assert.ok(requestOverview < 1040, 'request opens Overview by default');
      await detailClick(1114, 197);
      const timingTab = activeDetailTab(await layout('request-timing'));
      assert.ok(timingTab > requestOverview + 70, 'request Timing tab activates');
      await detailClick(600, 205);
      const toolTiming = activeDetailTab(await layout('tool-timing'));
      assert.ok(toolTiming > timingTab + 70, 'tool restores its compatible Timing tab');
      await detailClick(1068, 197);
      const toolInput = activeDetailTab(await layout('tool-arguments'));
      assert.ok(toolInput > requestOverview + 30 && toolInput < timingTab, 'tool opens the full Arguments tab');
      await page.keyboard.press('Tab');
      await page.keyboard.press('Enter');
      await page.waitForTimeout(250);
      const toolOutput = activeDetailTab(await layout('tool-result-keyboard'));
      assert.ok(toolOutput > toolInput + 30 && toolOutput < toolTiming, 'Tab and Enter activate Result');
      await page.keyboard.press('Shift+Tab');
      await page.keyboard.press('Space');
      await page.waitForTimeout(250);
      assert.equal(activeDetailTab(await layout('tool-arguments-keyboard')), toolInput, 'Shift+Tab and Space return to Arguments');
      const tree = page.getByRole('tree', { name: 'JSON 内容' });
      await writeFile(join(artifacts, 'json-dom.json'), JSON.stringify(await page.evaluate(() => [...document.querySelectorAll('.trajectory-json-frame')].map(node => ({ html: node.outerHTML, height: node.firstChild.getBoundingClientRect().height, hidden: node.hidden }))), null, 2));
      await tree.waitFor({ state: 'visible', timeout: 5000 });
      const treeStyle = await tree.evaluate(node => {
        const style = getComputedStyle(node.parentElement.parentElement);
        return { fontSize: style.fontSize, lineHeight: style.lineHeight };
      });
      assert.deepEqual(treeStyle, { fontSize: '12px', lineHeight: '16px' }, 'official JSON typography');
      const optionsNode = tree.getByRole('treeitem').filter({ has: page.locator(':scope > span').filter({ hasText: /^options:/ }) }).first();
      const expander = optionsNode.locator(':scope > [data-json-expander]');
      await expander.focus();
      await page.keyboard.press('ArrowRight');
      await page.waitForTimeout(120);
      assert.equal(await optionsNode.getAttribute('aria-expanded'), 'true', 'ArrowRight expands object');
      assert.ok(await tree.getByText('cwd:', { exact: true }).isVisible(), 'expanded JSON shows child fields');
      await page.keyboard.press('ArrowDown');
      assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-expanded')), 'false', 'ArrowDown reaches next visible expander');
      await page.keyboard.press('ArrowUp');
      await page.keyboard.press('ArrowLeft');
      assert.equal(await optionsNode.getAttribute('aria-expanded'), 'false', 'ArrowLeft collapses object');
      const commandRow = tree.getByRole('treeitem').filter({ hasText: /^command:/ }).first();
      await commandRow.hover();
      const copy = commandRow.getByRole('button', { name: '复制值', exact: true });
      await copy.click({ button: 'right' });
      const copyMenu = page.getByRole('menu');
      await copyMenu.waitFor({ state: 'visible' });
      await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
      await copyMenu.getByRole('menuitem', { name: '复制属性路径', exact: true }).click();
      assert.equal(await page.evaluate(() => navigator.clipboard.readText()), '$.command', 'copy menu writes exact JSON path');
      const longRow = tree.getByRole('treeitem').filter({ has: page.getByText('note:', { exact: true }) }).first();
      await longRow.getByRole('button', { name: '展开', exact: true }).click();
      assert.equal(await longRow.locator('pre').textContent(), '长文本保留\n'.repeat(240) + 'END\n', 'expanded string retains newlines including trailing newline');
      assert.equal(await longRow.locator('pre').getAttribute('data-wrap'), 'false', 'new installations default to unwrapped raw strings');
      await longRow.getByRole('button', { name: '自动换行' }).click();
      assert.equal(await longRow.locator('pre').getAttribute('data-wrap'), 'true');
      assert.equal(await page.evaluate(() => localStorage.getItem('dsh.trajectory.jsonStringWrapping')), 'true');
      await longRow.getByRole('button', { name: '收起', exact: true }).click();
      await longRow.getByRole('button', { name: '展开', exact: true }).click();
      assert.equal(await longRow.locator('pre').getAttribute('data-wrap'), 'true', 'next expansion samples the saved wrap preference');
      await longRow.getByRole('button', { name: '收起', exact: true }).click();
      await layout('json-tree');
      const divider = page.getByRole('separator', { name: '调整事件详情宽度' });
      const initialDivider = await divider.boundingBox();
      await divider.focus();
      await page.keyboard.press('ArrowLeft');
      await page.waitForFunction(x => Math.abs(document.querySelector('.trajectory-detail-resize').getBoundingClientRect().x - x) < 1, initialDivider.x - 16, { timeout: 5000 });
      assert.ok(Math.abs((await divider.boundingBox()).x - initialDivider.x + 16) < 1, `Left widens inspector by 16px: before=${JSON.stringify(initialDivider)}, after=${JSON.stringify(await divider.boundingBox())}`);
      await page.keyboard.press('ArrowRight');
      await page.waitForFunction(x => Math.abs(document.querySelector('.trajectory-detail-resize').getBoundingClientRect().x - x) < 1, initialDivider.x, { timeout: 5000 });
      assert.ok(Math.abs((await divider.boundingBox()).x - initialDivider.x) < 1, 'Right restores previous width');
      await page.mouse.move(initialDivider.x + 4, initialDivider.y + 50);
      await page.mouse.down();
      await page.mouse.move(initialDivider.x - 96, initialDivider.y + 50, { steps: 12 });
      await page.mouse.up();
      await page.waitForFunction(x => Math.abs(document.querySelector('.trajectory-detail-resize').getBoundingClientRect().x - x) < 1, initialDivider.x - 100, { timeout: 5000 });
      assert.ok(Math.abs((await divider.boundingBox()).x - initialDivider.x + 100) < 1, 'drag widens inspector by pointer delta');
      await layout('inspector-resized');
      await divider.dblclick();
      await page.waitForFunction(x => Math.abs(document.querySelector('.trajectory-detail-resize').getBoundingClientRect().x - x) < 1, initialDivider.x, { timeout: 5000 });
      assert.ok(Math.abs((await divider.boundingBox()).x - initialDivider.x) < 1, 'double click resets default width');
      await detailClick(600, 175);
      assert.ok(activeDetailTab(await layout('assistant-overview')) < 1040, 'message falls back to its most recent compatible tab');
      await detailClick(1124, 197);
      const rawTab = activeDetailTab(await layout('assistant-raw'));
      assert.ok(rawTab > 1100, 'assistant exposes original source blocks');
      await detailClick(600, 145);
      assert.equal(activeDetailTab(await layout('user-raw')), rawTab, 'next message preserves the Raw tab');
      await detailClick(1176, 197);
      const sourceTree = page.getByRole('tree', { name: 'JSON 内容' });
      await sourceTree.waitFor({ state: 'visible', timeout: 5000 });
      const sourceText = await sourceTree.textContent();
      assert.ok(sourceText.indexOf('kind:') < sourceText.indexOf('zeta:') && sourceText.indexOf('zeta:') < sourceText.indexOf('alpha:'), 'Source JSON retains upstream field order after the WASM projection');
      await layout('user-source-order');
      await page.keyboard.press('Escape');
      await page.waitForTimeout(292);
      await page.mouse.click(600, 145 + trajectoryOffset, { clickCount: 2 });
      await page.waitForTimeout(292);
      const folded = await layout('folded');
      assert.ok(folded.rowTransition(ledgerTop + 29) >= 8 && folded.rowTransition(ledgerTop + 49) >= 8, 'the fold preserves the first 30px record and adds one 20px summary');
      assert.ok(folded.rowTransition(ledgerTop + 59) < 8, 'the folded assistant and tool records no longer occupy content rows');
      await page.keyboard.press('Tab');
      await page.waitForTimeout(100);
      await page.screenshot({ path: join(artifacts, 'summary-focused.png') });
      await page.keyboard.press('Enter');
      await page.waitForTimeout(292);
      const enterExpanded = await layout('keyboard-enter');
      const contentEdges = layout => layout.edges.filter(y => y >= 160 && !layout.edges.includes(y - 2)).slice(0, 3);
      assert.deepEqual(contentEdges(enterExpanded), [ledgerTop + 29, ledgerTop + 59, ledgerTop + 89], 'Enter expands the focused summary');
      await page.mouse.click(600, 145 + trajectoryOffset, { clickCount: 2 });
      await page.waitForTimeout(200);
      await page.keyboard.press('Tab');
      await page.waitForTimeout(100);
      await page.keyboard.press('Space');
      await page.waitForTimeout(292);
      const spaceExpanded = await layout('keyboard-space');
      assert.deepEqual(contentEdges(spaceExpanded), [ledgerTop + 29, ledgerTop + 59, ledgerTop + 89], 'Space expands the focused summary');
      await toolbar.getByRole('button', { name: '收起所有调用', exact: true }).click();
      await page.waitForTimeout(250);
      assert.equal(await toolbar.getByRole('button', { name: '展开所有调用', exact: true }).getAttribute('aria-pressed'), 'true');
      const callsFolded = await layout('assistant-calls-folded');
      assert.ok(callsFolded.rowTransition(ledgerTop + 59) >= 8 && callsFolded.rowTransition(ledgerTop + 79) >= 8,
        'Calls keeps the assistant row and replaces its tool calls with a 20px summary');
      await search.fill('pwd');
      await page.waitForFunction(() => document.querySelectorAll('[data-search-match="true"]').length === 1);
      const matchingTool = await layout('search-inside-folded-calls');
      assert.ok(matchingTool.rowTransition(ledgerTop + 29) >= 8 && matchingTool.rowTransition(ledgerTop + 49) < 8,
        'search shows the matching call without its hidden parent or fold summary');
      await search.fill('');
      await page.waitForFunction(() => document.querySelectorAll('[data-search-match]').length === 0);
      assert.equal(await toolbar.getByRole('button', { name: '展开所有调用', exact: true }).getAttribute('aria-pressed'), 'true', 'clearing search preserves folds');
      await toolbar.getByRole('button', { name: '展开所有调用', exact: true }).click();
      await page.waitForTimeout(250);
      assert.deepEqual(contentEdges(await layout('calls-expanded')), [ledgerTop + 29, ledgerTop + 59, ledgerTop + 89]);
      await writeFile(join(artifacts, 'report.json'), JSON.stringify({ mode, expanded, folded, errors }, null, 2));
    }
    // AXPress invokes the DOM click without physical hit testing. These
    // measured controls must not add a second set of stops to the GPUI order.
    const accessible = page.locator('[data-trajectory-accessibility="controls"]');
    if (await page.getByRole('button', { name: '关闭详情', exact: true }).count()) {
      await page.getByRole('button', { name: '关闭详情', exact: true }).evaluate(node => node.click());
    }
    const toolAction = accessible.getByRole('button', { name: /^#\d+ 工具 bash/ });
    if (mode === 'trajectory') {
      await page.setViewportSize({ width: 1280, height: 820 });
    }
    await toolAction.waitFor();
    assert.equal(await toolAction.getAttribute('tabindex'), '-1');
    await toolAction.evaluate(node => node.click());
    const detailTabs = page.getByRole('tablist', { name: '事件详情', exact: true });
    await detailTabs.getByRole('tab', { name: '参数', exact: true }).waitFor();
    if (mode === 'trajectory-narrow') {
      assert.equal(await accessible.getByRole('button', { name: /^#\d+ / }).count(), 3, 'nonmodal drawer keeps exposed records accessible');
      const exposed = await toolAction.boundingBox();
      assert.equal(exposed.x + exposed.width, 200, 'record accessibility bounds stop at the drawer edge');
    } else {
      assert.equal(await toolAction.count(), 1, 'compact rows in a split pane remain accessible at 1280px');
    }
    await detailTabs.getByRole('tab', { name: '参数', exact: true }).evaluate(node => node.click());
    await page.waitForFunction(() => document.querySelector('[data-trajectory-accessibility="detail"] [aria-label="参数"]')?.getAttribute('aria-selected') === 'true');
    await page.waitForFunction(() => document.querySelectorAll('.trajectory-json-frame:not([hidden]) [role="tree"]').length === 1);
    await page.getByRole('tree', { name: 'JSON 内容', exact: true }).waitFor();
    await detailTabs.getByRole('tab', { name: '计时', exact: true }).focus();
    await page.keyboard.press('Tab');
    await page.waitForFunction(() => document.activeElement?.closest('.trajectory-json-frame'));
    await detailTabs.getByRole('tab', { name: '概述', exact: true }).evaluate(node => node.click());
    await accessible.getByRole('button', { name: '助手消息', exact: true }).waitFor();
    await accessible.getByRole('button', { name: '助手消息', exact: true }).evaluate(node => node.click());
    await accessible.getByRole('button', { name: '请求 #1', exact: true }).waitFor();
    const thinking = accessible.getByRole('button', { name: '思考', exact: true });
    assert.equal(await thinking.getAttribute('aria-expanded'), 'true', 'assistant reasoning starts expanded like upstream');
    await thinking.evaluate(node => node.click());
    await page.waitForFunction(() => document.querySelector('[data-trajectory-accessibility="controls"] [aria-label="思考"]')?.getAttribute('aria-expanded') === 'false');
    assert.equal(await detailTabs.getByRole('tab', { name: '概述', exact: true }).getAttribute('aria-selected'), 'true', 'hierarchy navigation forces Overview instead of restoring Arguments');
    await accessible.getByRole('button', { name: '请求 #1', exact: true }).evaluate(node => node.click());
    await detailTabs.getByRole('tab', { name: '用量', exact: true }).waitFor();
    assert.equal(await detailTabs.getByRole('tab', { name: '参数', exact: true }).count(), 0, 'assistant source opens the model request');
    await page.screenshot({ path: join(artifacts, 'hierarchy-request.png') });
    await page.getByRole('button', { name: '关闭详情', exact: true }).evaluate(node => node.click());
    const assistantAction = accessible.getByRole('button', { name: /^#\d+ 助手 Done/ });
    await assistantAction.waitFor();
    await assistantAction.evaluate(node => node.click());
    assert.equal(await thinking.getAttribute('aria-expanded'), 'false', 'returning to the same assistant retains its disclosure');
    await detailTabs.getByRole('tab', { name: '原始内容', exact: true }).evaluate(node => node.click());
    await accessible.getByRole('button', { name: '打开块 #3 的概要', exact: true }).waitFor();
    await accessible.getByRole('button', { name: '打开块 #3 的概要', exact: true }).evaluate(node => node.click());
    await detailTabs.getByRole('tab', { name: '参数', exact: true }).waitFor();
    assert.equal(await detailTabs.getByRole('tab', { name: '概述', exact: true }).getAttribute('aria-selected'), 'true', 'raw call block opens its tool summary');
    await detailTabs.getByRole('tab', { name: '计时', exact: true }).evaluate(node => node.click());
    await accessible.getByRole('button', { name: '显示 Unix 时间戳', exact: true }).waitFor();
    await accessible.getByRole('button', { name: '显示 Unix 时间戳', exact: true }).evaluate(node => node.click());
    await accessible.getByRole('button', { name: '显示本地时间', exact: true }).waitFor();
    await accessible.getByRole('button', { name: '显示本地时间', exact: true }).evaluate(node => node.click());
    await page.getByRole('button', { name: '关闭详情', exact: true }).evaluate(node => node.click());
    await detailTabs.waitFor({ state: 'detached' });
    await toolAction.waitFor();
    await search.fill('no-such-trajectory-record');
    await page.waitForFunction(() => ![...document.querySelectorAll('[data-trajectory-accessibility="controls"] button')].some(node => /^#\d+ /.test(node.getAttribute('aria-label') ?? '')));
    await search.fill('');
    await toolAction.waitFor();
    await page.getByRole('tab', { name: '对话', exact: true }).evaluate(node => node.click());
    await accessible.waitFor({ state: 'detached' });
    await page.getByRole('tab', { name: '轨迹', exact: true }).evaluate(node => node.click());
    await toolAction.waitFor();
    await page.screenshot({ path: join(artifacts, 'native-accessible-controls.png') });
    assert.equal(errors.length, 0, `no browser errors: ${errors.join('\n')}`);
  } else if (mode === 'inherited') {
    await page.waitForTimeout(600);
    await page.screenshot({ path: join(artifacts, 'inherited-dark.png') });
  } else if (mode === 'image') {
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'read_attachment'), null, { timeout: 20000 });
    await page.waitForTimeout(2000);
    await page.screenshot({ path: join(artifacts, 'image-initial.png') });
    const attachmentCalls = await page.evaluate(() => window.__verification.calls.filter(call => call.name === 'read_attachment'));
    assert.equal(attachmentCalls.length, 1, 'one durable image reference loads exactly once');
    await page.mouse.click(700, 158);
    await page.waitForTimeout(500);
    await page.screenshot({ path: join(artifacts, 'image-expanded.png') });
    await page.mouse.click(538, 232);
    await page.waitForTimeout(500);
    await page.screenshot({ path: join(artifacts, 'image-viewer.png') });
    await page.mouse.click(1390, 40);
    await page.waitForTimeout(500);
    await page.screenshot({ path: join(artifacts, 'image-viewer-closed.png') });
  } else if (mode === 'stream') {
    await page.waitForTimeout(2000);
    await page.screenshot({ path: join(artifacts, 'settled-before-live.png') });
    await page.mouse.click(790, 1027);
    await page.waitForTimeout(300);
    await page.mouse.click(790, 983);
    await page.waitForTimeout(300);
    await page.keyboard.type('请计算质能关系');
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(artifacts, 'composer-draft.png') });
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'prompt' && call.args.prompt?.[0]?.text === '请计算质能关系'), null, { timeout: 5000 });
    await page.waitForTimeout(300);
    assert.equal((await page.evaluate(() => window.__verification.calls)).filter(call => call.name === 'prompt').length, 1, 'one Enter sends exactly one prompt');
    await page.screenshot({ path: join(artifacts, 'composer-submitted.png') });
    await page.evaluate(sessionId => {
      window.__verification.emit({ kind: 'session_update', sessionId, update: {
        sessionUpdate: 'agent_message_chunk', messageId: 'live-assistant',
        content: { type: 'text', text: '流式公式 $\\frac{a+b}{2}$ 暂时保留原文。' },
      } });
    }, sessionId);
    await page.waitForTimeout(1000);
    const liveFormula = await page.screenshot({ path: join(artifacts, 'streaming-literal.png'), clip: { x: 480, y: 270, width: 430, height: 65 } });
    await page.evaluate(sessionId => {
      window.__verification.emit({ kind: 'session_update', sessionId, update: {
        sessionUpdate: 'turn_end', turn: 1, reason: 'completed', reasonObject: { kind: 'completed' },
      } });
      window.__verification.completePrompt();
    }, sessionId);
    await page.waitForTimeout(3000);
    const settledFormula = await page.screenshot({ path: join(artifacts, 'settled-after-live.png'), clip: { x: 480, y: 270, width: 430, height: 65 } });
    assert.notDeepEqual(liveFormula, settledFormula, 'the live formula changes when its turn settles');
    await writeFile(join(artifacts, 'stream-calls.json'), JSON.stringify(await page.evaluate(() => window.__verification.calls), null, 2));
  } else if (mode === 'appearance') {
    page.setDefaultTimeout(10000);
    const shell = page.locator('#settings-shell');
    const theme = page.locator('#settings-theme');
    const increase = theme.getByRole('button', { name: '增大字号', exact: true });
    const decrease = theme.getByRole('button', { name: '减小字号', exact: true });
    const frames = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    const open = async () => {
      await page.getByRole('button', { name: '更多设置', exact: true }).dispatchEvent('click');
      await page.getByRole('button', { name: '设置', exact: true }).dispatchEvent('click');
      await shell.getByRole('button', { name: '通用设置', exact: true }).click();
      await theme.getByRole('button', { name: '浅色', exact: true }).waitFor();
      await frames();
    };
    const saved = (themeMode, contentFontDelta) => page.waitForFunction(({ themeMode, contentFontDelta }) =>
      (() => { const value = window.__verification.calls.filter(call => call.name === 'save_workspace_preferences').at(-1)?.args.preferences;
        return value?.themeMode === themeMode && value?.contentFontDelta === contentFontDelta; })(), { themeMode, contentFontDelta }).catch(async error => {
      await writeFile(join(artifacts, 'failure-state.json'), JSON.stringify(await page.evaluate(() => ({ active: document.activeElement?.outerHTML, theme: document.getElementById('settings-theme')?.outerHTML, calls: window.__verification.calls })), null, 2));
      await page.screenshot({ path: join(artifacts, 'failure.png') }); throw error;
    });
    await open();
    assert.equal(await shell.getByRole('button', { name: '外观', exact: true }).count(), 0, 'appearance belongs to General, without a second navigation page');
    assert.equal(await theme.getByRole('button', { name: '浅色', exact: true }).getAttribute('aria-pressed'), 'true');
    const sizes = await theme.locator('button[aria-pressed]').evaluateAll(nodes => nodes.map(node => ({
      width: node.getBoundingClientRect().width, height: node.getBoundingClientRect().height,
      radius: getComputedStyle(node).borderRadius, top: node.getBoundingClientRect().top,
    })));
    assert.deepEqual(sizes.map(item => item.height), [84, 84, 84]); // Chromium DPR 1 snaps each 0.5px border to 1px.
    assert.equal(new Set(sizes.map(item => item.top)).size, 1, 'wide Mac panel has one row of theme cubes');
    assert.ok(sizes.every(item => item.radius === '20px'));
    assert.equal((await theme.boundingBox()).width, 564);
    await page.screenshot({ path: join(artifacts, 'general-light.png') });
    await theme.getByRole('button', { name: '深色', exact: true }).click();
    await saved('dark', 0);
    await increase.focus();
    assert.equal(await increase.evaluate(node => getComputedStyle(node.parentElement).opacity), '1', 'keyboard focus reveals the official stepper arrows');
    for (let index = 0; index < 8; index++) {
      await increase.press('Enter'); await saved('dark', index + 1);
    }
    await page.waitForFunction(() => document.querySelector('#settings-theme button[aria-label="增大字号"]')?.disabled);
    assert.equal(await increase.isDisabled(), true, 'official upper limit is 22px');
    await page.screenshot({ path: join(artifacts, 'general-dark-large.png') });
    await decrease.focus();
    for (let index = 7; index >= -4; index--) {
      await decrease.press('Enter'); await saved('dark', index);
    }
    await page.waitForFunction(() => document.querySelector('#settings-theme button[aria-label="减小字号"]')?.disabled);
    assert.equal(await decrease.isDisabled(), true, 'official lower limit is 10px');
    await theme.getByRole('button', { name: '跟随系统', exact: true }).click();
    await saved('system', -4);
    await theme.getByRole('button', { name: '跟随系统', exact: true }).press('Escape');
    await shell.waitFor({ state: 'hidden' }); await theme.waitFor({ state: 'hidden' });
    await page.reload();
    await page.waitForFunction(() => document.querySelector('canvas') && !document.getElementById('boot'), null, { timeout: 60000 });
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'turn_token_usage'));
    await open();
    assert.equal(await theme.getByRole('button', { name: '跟随系统', exact: true }).getAttribute('aria-pressed'), 'true', 'selection follows the persisted preference, not the system-resolved color');
    assert.equal(await decrease.isDisabled(), true, '10px survives a restart');
    await page.setViewportSize({ width: 620, height: 820 }); await frames();
    await page.waitForFunction(() => document.querySelector('#settings-theme')?.getBoundingClientRect().width === 336);
    const narrow = await theme.locator('button[aria-pressed]').evaluateAll(nodes => nodes.map(node => node.getBoundingClientRect().toJSON()));
    assert.equal(new Set(narrow.map(item => item.y)).size, 3, 'Mac keeps the official wrapping rule at narrow widths');
    assert.ok(narrow.every(item => item.width === 336));
    await page.screenshot({ path: join(artifacts, 'general-narrow.png') });
    const themeY = (await theme.boundingBox()).y;
    const target = await theme.getByRole('button', { name: '浅色', exact: true }).boundingBox();
    await page.mouse.move(target.x + target.width / 2, target.y + target.height / 2);
    await page.mouse.wheel(0, 160);
    await page.waitForFunction(y => document.querySelector('#settings-theme').getBoundingClientRect().y < y - 60, themeY);
    assert.equal(await shell.getByRole('button', { name: '关闭设置', exact: true }).isVisible(), true);
    const panelBox = await shell.boundingBox();
    assert.equal(await page.evaluate(({ x, y }) => document.elementsFromPoint(x, y).some(node => node.closest('#settings-theme')),
      { x: target.x + target.width / 2, y: panelBox.y + 24 }), false, 'scrolling cannot paint or hit theme controls over the fixed header');
    assert.equal(await page.evaluate(({ x, y }) => document.elementsFromPoint(x, y).some(node => node.closest('#settings-theme')),
      { x: target.x + target.width / 2, y: panelBox.y - 12 }), false, 'theme controls stay clipped inside the settings panel');
    await page.screenshot({ path: join(artifacts, 'general-narrow-scrolled.png') });
    await theme.getByRole('button', { name: '浅色', exact: true }).focus();
    await page.waitForFunction(top => {
      const node = document.querySelector('#settings-theme button[aria-pressed]');
      return node && node.getBoundingClientRect().top >= top;
    }, panelBox.y + 54);
    await page.screenshot({ path: join(artifacts, 'general-narrow-keyboard-reveal.png') });
    const lastGeneralControl = page.locator('#settings-developer').getByRole('switch');
    await lastGeneralControl.focus();
    await lastGeneralControl.press('Tab');
    assert.equal(await shell.getByRole('navigation').getByRole('button').first().evaluate(node => node === document.activeElement), true,
      'General Tab stays inside the settings dialog');
    await page.keyboard.press('Shift+Tab');
    assert.equal(await lastGeneralControl.evaluate(node => node === document.activeElement), true, 'reverse traversal returns to the last enabled General control');
    await shell.getByRole('navigation').getByRole('button', { name: '模型', exact: true }).click();
    await theme.waitFor({ state: 'hidden' });
    await shell.getByRole('button', { name: '关闭设置', exact: true }).click();
    await shell.waitFor({ state: 'hidden' });
    await writeFile(join(artifacts, 'appearance-calls.json'), JSON.stringify(await page.evaluate(() => window.__verification.calls), null, 2));
    assert.equal(errors.length, 0, `no browser errors: ${errors.join('\n')}`);
  } else if (['settings', 'settings-partial', 'settings-readonly', 'settings-search', 'settings-fetch'].includes(mode)) {
    let step = '';
    try {
    const shell = page.locator('#settings-shell');
    await page.getByRole('button', { name: '更多设置', exact: true }).dispatchEvent('click');
    await page.getByRole('button', { name: '设置', exact: true }).dispatchEvent('click');
    await shell.waitFor();
    await page.screenshot({ path: join(artifacts, 'credentials.png') });
    await shell.getByRole('button', { name: '网页搜索', exact: true }).click();
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings'), null, { timeout: 20000 });
    await page.waitForTimeout(1500);
    await page.screenshot({ path: join(artifacts, 'web-search.png') });
    const settingsCalls = await page.evaluate(() => window.__verification.calls);
    await writeFile(join(artifacts, 'settings-calls.json'), JSON.stringify(settingsCalls, null, 2));
    assert.equal(settingsCalls.filter(call => call.name === 'web_settings').length, 1, 'settings navigation requests the snapshot once');
    await shell.getByRole('button', { name: '通用设置', exact: true }).click();
    await page.waitForTimeout(300);
    await page.locator('#settings-theme').getByRole('button', { name: '深色', exact: true }).click();
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'save_workspace_preferences' && call.args.preferences.themeMode === 'dark' && call.args.preferences.contentFontDelta === 0), null, { timeout: 5000 });
    await shell.getByRole('button', { name: '网页搜索', exact: true }).click();
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(artifacts, 'web-search-dark.png') });
    if (mode === 'settings-readonly') {
      await page.screenshot({ path: join(artifacts, 'readonly.png') });
      await page.mouse.click(810, 595);
      await page.keyboard.press('ControlOrMeta+A');
      await page.keyboard.type('25');
      await page.keyboard.press('Tab');
      await page.mouse.click(560, 787);
      await page.waitForTimeout(300);
      assert.equal((await page.evaluate(() => window.__verification.calls)).filter(call => call.name === 'web_settings' && call.args.change).length, 0, 'read-only settings cannot be saved');
    } else if (mode === 'settings-search') {
      await page.mouse.click(810, 301);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'provider-menu.png') });
      await page.mouse.click(680, 427);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'legacy.png') });
      await page.mouse.click(560, 462);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-official'), null, { timeout: 5000 });
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'legacy-saved.png') });
      await page.mouse.click(810, 301);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'restore-menu.png') });
      await page.mouse.click(680, 388);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'restored.png') });
      await page.mouse.click(560, 787);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-search-api'), null, { timeout: 5000 });
      await page.waitForTimeout(700);
      await page.mouse.click(810, 427);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'billing-menu.png') });
      await page.mouse.click(680, 517);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'billing-account.png') });
      await page.mouse.click(560, 768);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.searchBilling === 'account'), null, { timeout: 5000 });
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'billing-saved.png') });
    } else if (mode === 'settings-fetch') {
      step = 'open-fetch-provider';
      await page.mouse.click(810, 732);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'provider-menu.png') });
      step = 'select-fetch-api';
      await page.mouse.click(680, 855);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'fetch-api.png') });
      step = 'save-fetch-api';
      await page.mouse.move(900, 850);
      await page.mouse.wheel(0, 700);
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(artifacts, 'fetch-api-scrolled.png') });
      await page.mouse.click(560, 891);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.fetch === 'deepseek-fetch-api'), null, { timeout: 5000 });
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'fetch-api-saved.png') });
      step = 'open-fetch-billing';
      await page.mouse.click(810, 783);
      await page.waitForTimeout(500);
      await page.keyboard.press('Home');
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(artifacts, 'billing-menu.png') });
      step = 'select-fetch-account';
      await page.keyboard.press('Enter');
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'billing-account.png') });
      step = 'save-fetch-account';
      await page.mouse.click(560, 891);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.fetchBilling === 'account'), null, { timeout: 5000 });
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'billing-saved.png') });
      const changes = (await page.evaluate(() => window.__verification.calls)).filter(call => call.name === 'web_settings' && call.args.change);
      assert.equal(changes.filter(call => call.args.change.fetch === 'deepseek-fetch-api').length, 1, 'fetch provider saves exactly once');
      assert.equal(changes.filter(call => call.args.change.fetchBilling === 'account').length, 1, 'fetch billing saves exactly once');
      await page.mouse.click(560, 891);
      await page.waitForTimeout(400);
      assert.equal((await page.evaluate(() => window.__verification.calls)).filter(call => call.name === 'web_settings' && call.args.change).length, changes.length, 'unchanged settings never save');
    } else if (mode === 'settings-partial') {
      const save = async () => {
        await page.mouse.move(900, 850);
        await page.mouse.wheel(0, 700);
        await page.waitForTimeout(500);
        await page.mouse.click(1068, 891);
        await page.waitForTimeout(300);
      };
      step = 'select-search-legacy';
      await page.mouse.click(810, 301);
      await page.waitForTimeout(300);
      await page.mouse.click(680, 427);
      await page.waitForTimeout(300);
      await page.mouse.click(560, 462);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-official'), null, { timeout: 5000 });
      await page.waitForTimeout(1200);
      step = 'select-search-api';
      await page.mouse.click(810, 301);
      await page.waitForTimeout(300);
      await page.mouse.click(680, 388);
      await page.waitForTimeout(400);
      await page.screenshot({ path: join(artifacts, 'restored.png') });
      step = 'select-fetch-api';
      await page.mouse.click(810, 732);
      await page.waitForTimeout(300);
      await page.mouse.click(680, 855);
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(artifacts, 'pending-changes.png') });
      step = 'save-partial';
      await save();
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.fetch === 'deepseek-fetch-api'), null, { timeout: 5000 });
      await page.waitForFunction(() => {
        const changes = window.__verification.calls.filter(call => call.name === 'web_settings' && call.args.change);
        return changes.length >= 2;
      }, null, { timeout: 5000 });
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'partial-failure.png') });
      step = 'partial-draft-preservation';
      await page.mouse.move(900, 200);
      await page.mouse.wheel(0, -1000);
      await page.waitForTimeout(400);
      await page.mouse.click(810, 301);
      await page.waitForTimeout(300);
      await page.keyboard.press('End');
      await page.waitForTimeout(200);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(artifacts, 'failed-fetch-draft.png') });
      await page.mouse.click(810, 732);
      await page.waitForTimeout(300);
      await page.keyboard.press('Home');
      await page.waitForTimeout(200);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(300);
      await save();
      await page.waitForFunction(() => window.__verification.calls.filter(call => call.name === 'web_settings' && call.args.change?.fetch === 'deepseek-fetch-api').length >= 2, null, { timeout: 5000 });
    } else {
      const save = async () => {
        await page.mouse.click(560, 787);
        await page.waitForTimeout(300);
      };
      step = 'invalid-count';
      await page.keyboard.press('ControlOrMeta+A');
      await page.keyboard.type('0');
      await page.keyboard.press('Tab');
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(artifacts, 'invalid-count.png') });
      step = 'save-invalid-count';
      await page.mouse.click(560, 787);
      await page.waitForTimeout(300);
      assert.equal((await page.evaluate(() => window.__verification.calls)).filter(call => call.name === 'web_settings' && call.args.change).length, 0, 'invalid numeric input never reaches IPC');
      step = 'valid-count';
      await page.mouse.click(810, 595);
      await page.keyboard.press('ControlOrMeta+A');
      await page.keyboard.type('25');
      await page.keyboard.press('Tab');
      await page.waitForTimeout(300);
      step = 'save-valid-count';
      await page.mouse.click(560, 787);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.fields?.count?.value === 25), null, { timeout: 5000 });
      await page.waitForTimeout(1000);
      await page.mouse.click(810, 301);
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'search-providers.png') });
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(300);
      await page.keyboard.press('End');
      await page.waitForTimeout(100);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'search-legacy.png') });
      await page.waitForTimeout(500);
      step = 'save-legacy';
      await page.mouse.click(560, 462);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-official'), null, { timeout: 5000 });
      await page.waitForTimeout(700);
      await page.mouse.click(810, 301);
      await page.waitForTimeout(500);
      await page.mouse.click(680, 388);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'search-api-restored.png') });
      step = 'save-search';
      await save();
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-search-api'), null, { timeout: 5000 });
      step = 'save-search-account';
      await page.mouse.click(810, 427);
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'search-billing.png') });
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(300);
      await page.keyboard.press('Home');
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(100);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'search-billing-account.png') });
      await page.mouse.click(560, 768);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.searchBilling === 'account'), null, { timeout: 5000 });
      step = 'save-search-api-key';
      await page.mouse.click(810, 427);
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'search-billing-key-menu.png') });
      await page.mouse.click(680, 550);
      await page.waitForTimeout(500);

      await page.screenshot({ path: join(artifacts, 'search-billing-key.png') });
      await page.mouse.click(560, 768);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.searchBilling === 'api-key'), null, { timeout: 5000 });
      step = 'save-search-auto';
      await page.mouse.click(810, 427);
      await page.waitForTimeout(700);
      await page.mouse.click(680, 480);
      await page.waitForTimeout(500);
      await save();
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.searchBilling === 'auto'), null, { timeout: 5000 });
      step = 'save-fetch-provider';
      await page.mouse.move(900, 850);
      await page.mouse.wheel(0, 700);
      await page.waitForTimeout(400);
      await page.mouse.click(810, 732);
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'fetch-providers.png') });
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(300);
      await page.keyboard.press('End');
      await page.waitForTimeout(100);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'fetch-api.png') });
      await page.mouse.click(560, 940);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.fetch === 'deepseek-fetch-api'), null, { timeout: 5000 });
      step = 'save-fetch-account';
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(artifacts, 'fetch-billing-region.png') });
      await page.mouse.click(810, 832);
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'fetch-billing.png') });
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(300);
      await page.keyboard.press('Home');
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(100);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'fetch-billing-account.png') });
      await page.mouse.click(560, 922);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.fetchBilling === 'account'), null, { timeout: 5000 });
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(artifacts, 'settings-footer.png') });
      await writeFile(join(artifacts, 'settings-calls.json'), JSON.stringify(await page.evaluate(() => window.__verification.calls), null, 2));
      const changes = (await page.evaluate(() => window.__verification.calls)).filter(call => call.name === 'web_settings' && call.args.change);
      assert.equal(changes.filter(call => Object.keys(call.args.change).includes('fetchBilling')).length, 1, 'fetch billing is saved exactly once');
    }
    } catch (error) {
      await writeFile(join(artifacts, 'failed-step.json'), JSON.stringify({ step, calls: await page.evaluate(() => window.__verification.calls), events: await page.evaluate(() => window.__verification.events) }, null, 2));
      await page.screenshot({ path: join(artifacts, 'failed-step.png') });
      throw error;
    }
    await writeFile(join(artifacts, 'final-calls.json'), JSON.stringify(await page.evaluate(() => window.__verification.calls.filter(call => call.name === 'web_settings')), null, 2));
  } else if (mode !== 'theme' && !mode.startsWith('settings')) {
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'read_attachment'), null, { timeout: 20000 });
    await page.waitForTimeout(3000);
    await page.screenshot({ path: join(artifacts, 'settled-markdown.png') });
    await writeFile(join(artifacts, 'page-info.json'), JSON.stringify(await page.evaluate(() => ({
      title: document.title, canvas: [...document.querySelectorAll('canvas')].map(canvas => ({ width: canvas.width, height: canvas.height })),
      calls: window.__verification.calls,
    })), null, 2));
    const attachmentCalls = await page.evaluate(() => window.__verification.calls.filter(call => call.name === 'read_attachment'));
    assert.equal(attachmentCalls.length, 1, 'one durable image reference loads exactly once');
    await page.mouse.click(700, 584);
    await page.waitForTimeout(500);
    await page.screenshot({ path: join(artifacts, 'edit-expanded.png') });
    await page.mouse.click(550, 792);
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(artifacts, 'diff-expanded.png') });
    await page.mouse.click(700, 666);
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(artifacts, 'web-error-expanded.png') });
  }
  for (const [themeMode, contentFontDelta, label] of mode === 'inherited' ? [['light', 0, 'inherited-light']] : !['all', 'theme'].includes(mode) ? [] : [['dark', 4, 'dark-large-font'], ['light', -2, 'light-small-font']]) {
    const themed = await browser.newPage({ viewport: { width: 1440, height: 1100 }, deviceScaleFactor: 1 });
    await page.close();
    themed.on('console', message => {
      if (message.type() === 'error') {
        errors.push(message.text()); console.error('CONSOLE', message.text());
      }
    });
    themed.on('console', message => {
      if (message.type() === 'error') {
        errors.push(message.text()); console.error('CONSOLE', message.text());
      }
    });
    await themed.addInitScript(({ sessionId, cwd, history, png, attachmentId, themeMode, contentFontDelta }) => {
      window.__verification = { calls: [] };
      window.__TAURI__ = { core: { invoke: async (name, args = {}) => {
        args = JSON.parse(JSON.stringify(args, (_, value) => value instanceof Map ? Object.fromEntries(value) : value));
        window.__verification.calls.push({ name, args });
        switch (name) {
          case 'start_auth': case 'auth_status': return { apiKey: { configured: true } };
          case 'load_workspace_preferences': return { themeMode, addedWorkspaces: [cwd], workDetails: 'verbose', contentFontDelta, scratchSession: sessionId };
          case 'client_preferences': return { writable: true, namespaces: [
            { ns: 'ui-theme', revision: 0, value: { preference: themeMode, fontSize: 14 + contentFontDelta } },
            { ns: 'ui-settings', revision: 0, value: { enabled: false } },
          ] };
          case 'load_shortcuts': return { revision: 1, document: { schemaVersion: 1, profiles: {} }, error: null };
          case 'load_developer_tools': return false;
          case 'start_service': return { workspace: cwd, servicePath: '/fixture', agent: { agentInfo: { name: 'fixture' } }, configOptions: [{ id: 'model', currentValue: 'fixture-model' }] };
          case 'list_sessions': return { sessions: [{ sessionId, cwd, title: '公式与工具结果验收', createdAt: 1 }], nextCursor: null };
          case 'list_local_sessions': return { sessions: [] };
          case 'peek_session_titles': return { [sessionId]: '公式与工具结果验收' };
          case 'new_session': case 'resume_session': return { sessionId, cwd, configOptions: [{ id: 'model', currentValue: 'fixture-model' }], hostRunning: false };
          case 'load_session_history': return history;
          case 'turn_token_usage': return {};
          case 'permission_mode': return { mode: 'default' };
          case 'plan_mode': return { active: false };
          case 'question_state': return [];
          case 'session_commands': return { commands: [] };
          case 'inbox_state': return { items: [] };
          case 'goal_state': return { goal: null };
          case 'agent_terminal_watch': return [];
          case 'read_attachment': return { attachmentId, dataUrl: `data:image/png;base64,${png}` };
          case 'note_panic': console.error(`RUST PANIC ${args.message}`); return null;
          default: return [];
        }
      } }, event: { listen: () => Promise.resolve(() => {}) } };
      window.__TAURI_INTERNALS__ = { invoke: window.__TAURI__.core.invoke };
    }, { sessionId, cwd, history, png, attachmentId: imageAttachment.attachmentId, themeMode, contentFontDelta });
    await themed.goto(`http://127.0.0.1:${server.address().port}/`);
    await themed.bringToFront();
    await themed.waitForFunction(() => document.querySelector('canvas') && !document.getElementById('boot'), null, { timeout: 60000 });
    await themed.waitForFunction(() => window.__verification.calls.some(call => call.name === 'load_session_history'), null, { timeout: 20000 }).catch(async error => {
      console.error('THEMED_STARTUP', JSON.stringify(await themed.evaluate(() => ({ calls: window.__verification?.calls, tauri: !!window.__TAURI__, boot: document.getElementById('boot')?.textContent, url: location.href, state: document.readyState, visibility: document.visibilityState }))));
      throw error;
    });
    await themed.waitForFunction(({ themeMode, contentFontDelta }) => window.__verification.calls.some(call =>
      call.name === 'configure_shell_window'
      && call.args.dark === (themeMode === 'dark')
    ), { themeMode, contentFontDelta }, { timeout: 20000 }).catch(async error => {
      console.error('PREFERENCE_WAIT', JSON.stringify(await themed.evaluate(() => ({ calls: window.__verification?.calls, tauri: !!window.__TAURI__, boot: document.getElementById('boot')?.textContent, canvas: document.querySelector('canvas')?.getBoundingClientRect().toJSON() }))));
      throw error;
    });
    await themed.waitForTimeout(2000);
    await themed.screenshot({ path: join(artifacts, `${label}.png`) });
    await themed.close();
  }
  assert.deepEqual(errors, [], 'WASM and MathJax must execute without browser errors');
  const assertions = mode === 'plan-preview'
    ? ['logged and temporary plan previews', 'code copy and wrap', 'narrow scrollbar', 'restored-plan pending and missing states', 'review failure and retry']
    : mode === 'document-question'
    ? ['YAML field order and literal values', 'pagination failure and retry', 'document-relative images after EOF', 'recorded question reply from history', 'official question form', 'answer failure, retry and exact option label']
    : mode === 'agent-presets'
    ? ['preset cards and YAML', 'guide tabs and focus', 'default write conflict and retry', 'Creator draft and seat selection']
    : mode === 'theme'
    ? ['dark theme with an increased font size', 'light theme with a reduced font size']
    : mode === 'chat-markdown'
    ? ['official headings, tables, KaTeX, mixed inline wrapping', 'actual measured width and height beside preview', 'wheel routes to transcript', 'code wrapping', 'session DOM cleanup']
    : mode === 'file-links'
    ? ['ordinary Markdown HTML opens in preview', 'line navigation loads through page 81', 'same file reuses loaded document', 'outside-workspace path and percent-decoded filename', 'unsafe destination remains inert', 'source-line landing and fading mark screenshots']
    : mode === 'ui-feedback'
    ? ['unknown/nonblank history replay hides the composer without showing the hero', 'composer returns after replay', 'narrow mixed-link text and local HTML preview screenshots']
    : mode === 'trajectory-blocks'
    ? ['ordered mixed content and literal blocks', 'single-text-only JSON tree', 'official image size and lightbox', 'Escape and focus restoration', 'image context menu and clipboard', 'native save request and cancellation', 'retry after image read failure', 'file metadata and narrow layout']
    : mode === 'appearance'
    ? ['official appearance cubes and font stepper in General', 'rapid keyboard changes keep every step', '10–22px limits and persisted restoration', 'preference distinct from resolved theme', 'Mac narrow wrapping and scrolling', 'fixed viewport clipping and keyboard reveal', 'page change and Escape cleanup']
    : mode === 'settings-shell'
    ? ['shared 800px panel and 188px Mac rail', 'native navigation and selected-page focus', '28px configuration and close actions', 'Host-owned document availability', 'single-flight open, localized failure and retry', 'stale connection completion ignored']
    : mode === 'goal'
    ? ['multiline goal edit', 'composer draft preserved', 'single-flight save', 'failed save retry', 'stale update ignored']
    : mode === 'reading'
      ? ['background failure dot', 'no background acknowledgement', 'foreground acknowledgement']
    : mode === 'turn-navigation'
      ? ['turn marks at the wide chat edge', 'no turn marks in Trajectory or narrow Chat']
    : mode === 'trajectory'
      ? ['upstream equal/recorded duration bands and TTFT gradient', 'search-to-timeline Tab handoff', 'request and record inspectors', 'type-specific detail tabs', 'compatible tab restoration', 'keyboard tab activation', 'raw message blocks', 'upstream JSON tree typography and arrow navigation', 'JSON path copied through the real clipboard', 'complete raw strings and persisted wrapping', 'source JSON preserves field order', '16px keyboard resize, pointer drag and double-click reset', 'folding with Enter and Space']
    : mode === 'trajectory-results'
      ? ['live raw object and scalar results', 'bounded selection ownership', 'stale frame rejection', 'close and reconnect cleanup']
    : mode === 'trajectory-program'
      ? ['recorded program language', 'upstream CodeBlock and line numbers', 'source and original JSON copy', 'shared wrapping preference', 'structured output and literal failures', 'malformed and running calls']
    : mode === 'trajectory-prompts'
      ? ['historical system snapshots', 'system/tools/combined diffs', 'original diff line metrics', 'tool catalog keyboard disclosure', 'empty prompt and tool states']
    : mode === 'trajectory-narrow'
      ? ['compact ledger rows', 'right-side inspector drawer', 'Escape closes details', 'request boundary opens details']
    : mode === 'trajectory-menus'
      ? ['Retina session menu fully occludes the toolbar and timeline', 'dismissing the menu restores the timeline']
    : mode === 'trajectory-folds'
      ? ['canvas application menu preserves the toolbar and timeline outside its bounds', 'menu owns pointer hits in its bounds', 'assistant calls and tool subcalls use separate fold domains', 'turn fold preserves the first row', 'search bypasses all folds', 'clearing search restores folds', 'expansion preserves other domains']
    : mode === 'trajectory-timeline'
      ? ['upstream equal-width bands', 'inclusive focus without removing rows', 'Escape resets range', 'tooltip and block selection', 'inspector focus handoff', 'wheel zoom and right-drag pan']
    : mode === 'trajectory-long'
      ? ['virtualized long ledger', 'scroll to earlier records', 'tail following', 'composer clearance']
    : mode === 'trajectory-pages'
      ? ['loaded prefix is scrollable', 'single-flight older history', 'explicit retry after failure', 'metadata-only cursor advancement', 'stable viewport after prepend']
    : mode === 'session-order'
      ? ['pointer drag and Escape', 'separate grouped and flat orders', 'hidden archive positions', 'order restored after reload', 'manual order reset by recent sorting', 'scratch slot preserved after first prompt']
    : mode === 'workspace-registry'
      ? ['workspace drag and Escape', 'nested Workspace drop by Host identity', 'confirmed and rejected order writes', 'membership by Host identity', 'rejected and confirmed pin changes', 'external archive beats stale pin reply', 'directory picker uses registered canonical path and ID']
    : mode === 'workspace-actions'
      ? ['Host registry supersedes local aliases and hides', 'duplicate name validation', 'rename retry preserves draft', 'late rename cannot overwrite newer title', 'confirmed removal and retry', 'open a retained ungrouped session']
    : mode === 'workspace-startup'
      ? ['first-use workspace then one blank session', 'no automatic prompt', 'folder picker after initialization failure', 'history-only startup', 'removed default stays absent', 'manual picker supersedes pending startup', 'recent workspace selection', 'saved session restoration']
    : mode === 'welcome'
      ? ['native password field and trapped focus', 'model readiness before prompting', 'credential validation, masking and reveal', 'single-flight save and retry', 'focus after success', 'configure later and compatible provider settings', 'saved routes bypass setup']
    : mode === 'deepseek-account'
      ? ['welcome login with draft preservation', 'themed browser handoff and clipboard', 'sidebar keyboard menu and support link', 'shared avatar fallback and retry', 'account identity and wallet balances', 'embedded usage, retry, resize and native teardown before focus restoration', 'top-up refresh after a pending read and stale open response', 'confirmed sign-out and running-task recheck', 'sidebar dialogs and restored focus', 'saved login and stale profile response isolation', 'external usage link for older Hosts']
    : mode === 'desktop-onboarding'
      ? ['signed-in welcome and official artwork', 'no-credit confirmation, recharge and granted wallet', 'both purposes, rollback and retry', 'restored work-process choice', 'narrow layout', 'office, skip and API Key completion']
    : mode === 'account-notifications'
      ? ['first visible frame, no repeated display report', 'non-modal stack and plain text', 'settings action retains displayed card and refreshes the feed', 'localized copy keeps button focus', 'dismiss promotes the next card', 'credential replacement rejects stale replies', 'onboarding suppresses presentation']
    : mode === 'account-quota'
      ? ['live exact codes only', 'ordinary toast expires', 'dialog focus and Escape', 'deferred while the shared usage page is open', 'top-up ownership and concurrent failures', 'sign-out cleanup and legacy fallback', 'modal above onboarding']
    : mode === 'provider-authorization'
      ? ['Host login methods', 'device-code copy and external browser', 'choice, masked and manual prompts', 'stale status and withdrawn prompts', 'login and logout with draft retained', 'Escape cancels the matching attempt', 'failed login retries with fresh status polling']
    : mode === 'provider-protocols'
      ? ['protocol choices follow the Host', 'Anthropic preset and URL validation', 'Gemini mandatory same-origin credentials', 'Gemini fixed reasoning with protocol switch validation', 'discovery and saved Gemini models', 'shared route IDs and deployment-owned controls', 'stale edits preserve the draft and require a fresh profile selection', 'installed provider search, native authentication and endpoint hints', 'catalog discovery, model overrides, reset and removal']
    : mode === 'provider-settings'
      ? ['accessible provider setup from the welcome screen and settings', 'catalog retry and presets', 'address, protocol and native password', 'discovery error, empty response and late reply', 'candidate defaults, fuzzy search, filtered bulk selection, cancellation and focus', 'compact scrollable picker and staged updates preserving existing models', 'editable model ID, display name, capacities and input modalities', 'keyboard and drag ordering', 'reasoning defaults, ordered levels, custom values and inheritance', 'retry modes, codes, inherited limits and cross-field validation', 'validation and save retry', 'explicit override removal and preserved source ID', 'credential reset after save', 'named removal confirmation, cancel and retry']
    : mode === 'attachments'
      ? ['real File intake and two same-name files', 'bounded chunks, retry and staging cleanup', 'file-only idle submission and download card', 'empty file in the running queue', '/plan file admission and draft retention', 'accessible draft removal and exact file download']
    : mode === 'composer-focus'
      ? ['unbound IME hidden from accessibility', 'native composer focus restores real editor', 'native replacement updates the draft', 'normal send uses the updated draft']
    : mode === 'composer'
      ? ['busy Enter preference and failure rollback', 'preference after reload', 'Enter, primary and accelerated delivery', 'queue thumbnail and file identity', 'shared attachment cache', 'empty accelerated Enter steers the complete queue']
    : mode === 'queue'
      ? ['Enter queues; Cmd+Enter steers', 'pending echo, Host acceptance and early admission', 'accordion and inline edit', 'save retry, cancellation and composer draft preservation', 'stale snapshot, steering and deletion']
    : mode === 'approval'
      ? ['tool command and ACP option kinds', 'approval keys scoped to the card', 'IME and repeat guards', 'single-flight decision and retry', 'FIFO and exact request withdrawal', 'stale replies, reconnect, and persistent decisions']
    : mode === 'permission'
      ? ['permission load error and retry', 'risk acknowledgement and trapped focus', 'cancellation preserves search', 'change failure and retry', 'confirmed and ordinary writes', 'reselect current toolbar mode']
    : mode === 'team'
      ? ['team board and toggle', 'disable without model use', 'modal warnings and focus trap', 'keyboard and backdrop cancellation', 'late save from another session', 'failure and retry of the original session']
    : mode === 'model-catalog'
      ? ['catalog loading and full error', 'keyboard retry in both entries', 'single flight and stale topology response', 'usable provider selection during partial failure', 'retry restores the failed provider and preserves query', 'shared directory and last-good options in /model']
    : mode === 'model-accessibility'
      ? ['model trigger, current model and reasoning effort', 'keyboard search, active result and retry', 'announced provider and selection failures', 'command launcher and typed /model suggestions', 'checked model, failure, retry and focus return in /model']
    : mode === 'model-menu'
      ? ['model description and model ID matching', 'official provider order', 'single-flight selection and retry', 'provider-default and high reasoning effort', 'current selection no-op', 'focus across catalog changes and when no model is selected', 'shared model selection from /model']
    : mode === 'workspace-search'
      ? ['official fuzzy score and kana voicing', 'rank and recency before limit', 'mixed fuzzy and ID keys', 'live workspace label updates and durable blank exclusion', 'debounce and stale-reply cancellation', 'ranked content snippets and deduplication', 'empty and refine states', 'keyboard clear and navigation beyond the browsing watermark', 'archived open guard and in-place restoration', 'canvas text input, outside click and focus recovery']
    : mode === 'workspace-accessibility'
      ? ['tree roles and focus recovery', 'menu shortcut, disabled rows and submenu focus', 'captured Session pin', 'rename validation, error and retry', 'archive confirmation, cancel and stop']
    : mode === 'workspace-group-accessibility'
      ? ['keyboard menu and focus return', 'folder reveal', 'workspace rename validation and retry', 'stale workspace identity rejection', 'workspace removal confirmation and retry', 'retained sessions become ungrouped']
      : ['real WASM boot', 'history replay', 'MathJax loaded without errors', 'durable attachment loaded'];
  console.log(JSON.stringify({ assertions, artifacts }, null, 2));
} catch (error) {
  verificationFailed = true;
  console.error(error.stack ?? error);
  if (mode.startsWith('trajectory')) { await page.screenshot({ path: join(artifacts, 'trajectory-failed.png') }).catch(() => {}); }
  if (['welcome', 'provider-settings', 'provider-protocols', 'provider-authorization', 'deepseek-account', 'workspace-search', 'workspace-accessibility'].includes(mode)) {
    console.error(error.stack);
    await page.screenshot({ path: join(artifacts, `${mode}-failed.png`) }).catch(() => {});
  }
  if (['queue', 'composer', 'composer-focus', 'attachments'].includes(mode)) {
    console.error(error.stack);
    if (mode === 'attachments') {
      console.error(await page.evaluate(() => ({
        focus: document.activeElement?.id,
        editor: document.getElementById('gpui-text-input')?.value,
        calls: window.__verification.calls.slice(-8).map(call => [call.name, call.args.operation?.kind]),
      })).catch(() => null));
    }
    await page.screenshot({ path: join(artifacts, 'queue-failed.png') }).catch(() => {});
  }
  throw error;
} finally {
  let cleanupTimer;
  try {
    const graceful = browser.close().then(() => browserServer.close());
    const forced = new Promise((resolve, reject) => {
      cleanupTimer = setTimeout(() => {
        const child = browserServer.process();
        console.log('Stopping the owned test browser after its graceful-close deadline');
        void browserServer.kill().catch(reject);
        if (child.exitCode !== null || child.signalCode !== null) { resolve(); return; }
        const deadline = setTimeout(() => reject(new Error('owned test browser did not exit')), 5000);
        child.once('exit', () => { clearTimeout(deadline); resolve(); });
      }, 5000);
    });
    await Promise.race([graceful, forced]);
  } catch (error) {
    verificationFailed = true; console.error(error);
  } finally {
    clearTimeout(cleanupTimer);
    server.close(); server.closeAllConnections();
  }
  process.exit(verificationFailed ? 1 : 0);
}
