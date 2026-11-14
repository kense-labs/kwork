import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, extname, join, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const modeIndex = process.argv.indexOf('--mode');
const mode = modeIndex >= 0 ? process.argv[modeIndex + 1] : 'all';
const artifactArgument = process.argv.slice(2).find(argument => argument !== '--mode' && argument !== mode);
const artifacts = resolve(artifactArgument || join(root, '.tools/verification/markdown-tools-browser', mode));
await mkdir(artifacts, { recursive: true });
const sessionId = 'session-markdown-tools-browser';
const cwd = root;
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.png': 'image/png', '.svg': 'image/svg+xml', '.ttf': 'font/ttf' };
const server = createServer(async (request, response) => {
  try {
    const relative = decodeURIComponent(new URL(request.url, 'http://localhost').pathname).replace(/^\/+/, '') || 'index.html';
    const path = join(root, 'ui/dist', normalize(relative));
    if (!path.startsWith(join(root, 'ui/dist') + '/')) throw new Error('invalid path');
    response.writeHead(200, { 'Content-Type': mime[extname(path)] || 'application/octet-stream' });
    response.end(await readFile(path));
  } catch (error) {
    response.writeHead(404); response.end(String(error));
  }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const browser = await chromium.launch({
  executablePath: process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true,
  args: ['--no-first-run', '--enable-unsafe-webgpu', '--use-angle=swiftshader'],
});
const page = await browser.newPage({ viewport: { width: 1440, height: 1100 }, deviceScaleFactor: 1 });
const errors = [];
page.on('pageerror', error => { errors.push(String(error)); console.error('PAGE ERROR', error); });
page.on('console', message => {
  if (message.type() === 'error' && message.text() !== "[ERROR] : can't render at a zero size") {
    errors.push(message.text()); console.error('CONSOLE', message.text());
  }
  if (message.text().startsWith('web settings')) console.log('DIAGNOSTIC', message.text());
});
const png = (await readFile(join(root, 'src-tauri/icons/64x64.png'))).toString('base64');
const imageAttachment = { attachmentId: 'a'.repeat(64), mediaType: 'image/png', bytes: Buffer.from(png, 'base64').length, width: 64, height: 64, name: 'fixture.png' };
assert.ok(['all', 'image', 'theme', 'stream', 'trajectory', 'settings', 'settings-partial', 'settings-readonly', 'settings-search', 'settings-fetch', 'appearance', 'plugins', 'workflow', 'team'].includes(mode), `unknown mode: ${mode}`);
const turnStart = { type: 'turn/start', seq: 1, data: { turn: 1 } };
const userMessage = { type: 'user/message', seq: 2, surfaceOp: 'append', data: { id: 'user', source: { kind: 'user' }, role: 'user', content: [{ type: 'text', text: '请验证公式、差异和图片' }] } };
const turnEnd = { type: 'turn/end', seq: 10, data: { turn: 1, reason: { kind: 'completed' } } };
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
const events = mode === 'image'
  ? [turnStart, userMessage, ...imageEvents, turnEnd]
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
  : mode === 'trajectory'
    ? [
      { type: 'turn/start', seq: 1, time: 1000, data: { turn: 1 } },
      { type: 'user/message', seq: 2, time: 1100, data: { id: 'u', source: { kind: 'user' }, content: [{ type: 'text', text: 'Inspect this' }] } },
      { type: 'step/start', seq: 3, time: 1200, data: { turn: 1, step: 1 } },
      { type: 'assistant/message', seq: 4, time: 1600, data: { turn: 1, step: 1, message: { id: 'a', source: { provider: 'deepseek', model: 'v4' }, content: [{ type: 'reasoning', text: 'Think' }, { type: 'text', text: 'Done' }] }, usage: { inputTokens: 20, outputTokens: 5 } } },
      { type: 'tool/call', seq: 5, time: 1700, data: { turn: 1, step: 1, callId: 'c', name: 'bash', arguments: '{"command":"pwd"}' } },
      { type: 'tool/result', seq: 6, time: 1900, data: { turn: 1, step: 1, message: { source: { callId: 'c' }, content: [{ type: 'text', text: '/tmp' }] } } },
      { type: 'step/end', seq: 7, time: 2000, data: { turn: 1, step: 1 } },
      turnEnd,
    ]
  : ['theme', 'stream', 'settings', 'settings-partial', 'settings-readonly', 'settings-search', 'settings-fetch', 'appearance', 'plugins', 'workflow', 'team'].includes(mode)
    ? themeEvents
    : [turnStart, userMessage, ...contentEvents, ...imageEvents, turnEnd];
const history = { events, hasMore: false, sessionStats: null, tokenUsage: null };
try {
  await page.addInitScript(({ sessionId, cwd, history, png, attachmentId, mode }) => {
    const calls = [];
    const listeners = [];
    window.__verification = { calls, emit: payload => listeners.forEach(listener => listener({ event: 'dsh-gpui-event', payload })) };
    window.__TAURI__ = {
      core: { invoke: async (name, args = {}) => {
        args = JSON.parse(JSON.stringify(args, (_, value) => value instanceof Map ? Object.fromEntries(value) : value));
        calls.push({ name, args });
        switch (name) {
          case 'start_auth': case 'auth_status': return { apiKey: { configured: true } };
          case 'load_workspace_preferences': return { themeMode: mode === 'appearance' ? 'light' : 'dark', addedWorkspaces: [cwd], workDetails: 'verbose', contentFontDelta: 0, scratchSession: sessionId };
          case 'load_shortcuts': return { revision: 1, document: { schemaVersion: 1, profiles: {} }, error: null };
          case 'load_developer_tools': return false;
          case 'start_service': return { workspace: cwd, servicePath: '/fixture', agent: { agentInfo: { name: 'fixture' } }, configOptions: [] };
          case 'list_sessions': return { sessions: [{ sessionId, cwd, title: '公式与工具结果验收', createdAt: 1 }], nextCursor: null };
          case 'list_local_sessions': return { sessions: [] };
          case 'peek_session_titles': return { [sessionId]: '公式与工具结果验收' };
          case 'new_session': case 'resume_session': return { sessionId, cwd, configOptions: [], hostRunning: false };
          case 'load_session_history': return history;
          case 'turn_token_usage': return {};
          case 'prompt': return new Promise(resolve => {
            window.__verification.completePrompt = () => resolve({ stopReason: 'end_turn' });
          });
          case 'permission_mode': return { mode: 'default' };
          case 'plan_mode': return { active: false };
          case 'question_state': return [];
          case 'session_commands': return { commands: [] };
          case 'inbox_state': return { items: [] };
          case 'goal_state': return { goal: null };
          case 'team_state': {
            const current = { enabled: false, tasks: [{ id: 'task-api', subject: '实现 API', description: '补齐接口和测试', status: 'in_progress', ready: true, ownerId: 'child-api', ownerName: 'API Agent' }], hasUsedModel: false };
            if (typeof args.enabled === 'boolean') current.enabled = args.enabled;
            return { available: true, team: current, request: args.request };
          }
          case 'web_settings': {
            const snapshot = {
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
          case 'agent_terminal_watch': return [];
          case 'read_attachment': return { attachmentId, dataUrl: `data:image/png;base64,${png}` };
          case 'note_panic': console.error(`RUST PANIC ${args.message}`); return null;
          case 'configure_shell_window': case 'save_workspace_preferences': case 'session_read_ack':
          case 'document_system_font': case 'chinese_system_font': case 'list_shells': return [];
          default: console.log(`UNHANDLED_IPC ${name} ${JSON.stringify(args)}`); return null;
        }
      } },
      event: { listen: (name, listener) => { listeners.push(listener); return Promise.resolve(() => {}); } },
    };
    window.__TAURI_INTERNALS__ = { invoke: window.__TAURI__.core.invoke };
  }, { sessionId, cwd, history, png, attachmentId: imageAttachment.attachmentId, mode });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => document.querySelector('canvas') && !document.getElementById('boot'), { timeout: 60000 });
  await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'load_session_history'), { timeout: 20000 });
  await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'turn_token_usage'), { timeout: 20000 });
  if (mode === 'workflow') {
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
  } else if (mode === 'team') {
    await page.waitForTimeout(700);
    await page.screenshot({ path: join(artifacts, 'board.png') });
    await page.mouse.click(710, 979);
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(artifacts, 'menu.png') });
    await page.mouse.click(780, 855);
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(artifacts, 'pane.png') });
    await page.mouse.click(810, 720);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'team_state' && call.args.enabled === true), { timeout: 5000 });
    await page.waitForTimeout(500);
    await page.screenshot({ path: join(artifacts, 'enabled.png') });
  } else if (mode === 'plugins') {
    await page.mouse.click(50, 168);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'plugin_center'), { timeout: 10000 });
    await page.waitForTimeout(1500);
    await page.screenshot({ path: join(artifacts, 'plugin-badges.png') });
    await page.mouse.click(560, 305);
    await page.waitForTimeout(700);
    await page.screenshot({ path: join(artifacts, 'plugin-detail.png') });
  } else if (mode === 'trajectory') {
    await page.waitForTimeout(500);
    assert.equal((await page.evaluate(() => window.__verification.calls)).filter(call => ['new_session', 'prompt', 'queue_prompt'].includes(call.name)).length, 0, 'history remains read-only');
    await page.mouse.click(1318, 25);
    await page.waitForTimeout(700);
    await page.screenshot({ path: join(artifacts, 'trajectory.png') });
    await page.mouse.click(680, 213);
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(artifacts, 'folded.png') });
    await page.mouse.click(1010, 74);
    await page.keyboard.type('pwd');
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(artifacts, 'search.png') });
    await page.keyboard.press('Escape');
    await page.mouse.click(600, 565);
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(artifacts, 'inspector.png') });
    await page.mouse.click(1374, 79);
    await page.waitForTimeout(200);
    await page.keyboard.press('Escape');
    await page.waitForTimeout(200);
    await page.screenshot({ path: join(artifacts, 'inspector-closed.png') });
    await page.mouse.click(680, 218);
    await page.waitForTimeout(200);
    await page.keyboard.press('Space');
    await page.waitForTimeout(200);
    await page.screenshot({ path: join(artifacts, 'keyboard-fold.png') });
  } else if (mode === 'image') {
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'read_attachment'), { timeout: 20000 });
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
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'prompt' && call.args.prompt?.[0]?.text === '请计算质能关系'), { timeout: 5000 });
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
    await page.mouse.click(58, 1072);
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(artifacts, 'more-menu.png') });
    await page.mouse.click(130, 1020);
    await page.waitForTimeout(700);
    await page.screenshot({ path: join(artifacts, 'credentials.png') });
    await page.mouse.click(430, 322);
    await page.waitForTimeout(500);
    await page.screenshot({ path: join(artifacts, 'appearance.png') });
    for (let index = 0; index < 4; index++) {
      await page.mouse.click(1080, 371);
      await page.waitForTimeout(150);
    }
    await page.mouse.click(662, 298);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'save_workspace_preferences' && call.args.preferences.themeMode === 'dark' && call.args.preferences.contentFontDelta === 4), { timeout: 5000 });
    await page.screenshot({ path: join(artifacts, 'dark-large-font.png') });
    await writeFile(join(artifacts, 'appearance-calls.json'), JSON.stringify(await page.evaluate(() => window.__verification.calls), null, 2));
    await page.keyboard.press('Escape');
    await page.waitForTimeout(500);
    await page.screenshot({ path: join(artifacts, 'dark-conversation.png') });
  } else if (['settings', 'settings-partial', 'settings-readonly', 'settings-search', 'settings-fetch'].includes(mode)) {
    let step = '';
    try {
    await page.mouse.click(58, 1072);
    await page.waitForTimeout(300);
    await page.mouse.click(130, 1020);
    await page.waitForTimeout(700);
    await page.screenshot({ path: join(artifacts, 'credentials.png') });
    await page.mouse.click(430, 366);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings'), { timeout: 20000 });
    await page.waitForTimeout(1500);
    await page.screenshot({ path: join(artifacts, 'web-search.png') });
    const settingsCalls = await page.evaluate(() => window.__verification.calls);
    await writeFile(join(artifacts, 'settings-calls.json'), JSON.stringify(settingsCalls, null, 2));
    assert.equal(settingsCalls.filter(call => call.name === 'web_settings').length, 1, 'settings navigation requests the snapshot once');
    await page.mouse.click(430, 322);
    await page.waitForTimeout(300);
    await page.mouse.click(662, 298);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'save_workspace_preferences' && call.args.preferences.themeMode === 'dark' && call.args.preferences.contentFontDelta === 0), { timeout: 5000 });
    await page.mouse.click(430, 366);
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
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-official'), { timeout: 5000 });
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'legacy-saved.png') });
      await page.mouse.click(810, 301);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'restore-menu.png') });
      await page.mouse.click(680, 390);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'restored.png') });
      await page.mouse.click(560, 787);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-search-api'), { timeout: 5000 });
      await page.waitForTimeout(700);
      await page.mouse.click(810, 427);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'billing-menu.png') });
      await page.mouse.click(680, 517);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'billing-account.png') });
      await page.mouse.click(560, 787);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.searchBilling === 'account'), { timeout: 5000 });
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
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.fetch === 'deepseek-fetch-api'), { timeout: 5000 });
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
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.fetchBilling === 'account'), { timeout: 5000 });
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'billing-saved.png') });
      const changes = (await page.evaluate(() => window.__verification.calls)).filter(call => call.name === 'web_settings' && call.args.change);
      assert.equal(changes.filter(call => call.args.change.fetch === 'deepseek-fetch-api').length, 1, 'fetch provider saves exactly once');
      assert.equal(changes.filter(call => call.args.change.fetchBilling === 'account').length, 1, 'fetch billing saves exactly once');
      await page.mouse.click(560, 787);
      await page.waitForTimeout(400);
      assert.equal((await page.evaluate(() => window.__verification.calls)).filter(call => call.name === 'web_settings' && call.args.change).length, changes.length, 'unchanged settings never save');
    } else if (mode === 'settings-partial') {
      step = 'select-search-legacy';
      await page.mouse.click(810, 301);
      await page.waitForTimeout(300);
      await page.mouse.click(680, 427);
      await page.waitForTimeout(300);
      await page.mouse.click(560, 462);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-official'), { timeout: 5000 });
      await page.waitForTimeout(1200);
      step = 'select-search-api';
      await page.mouse.click(810, 301);
      await page.waitForTimeout(300);
      await page.keyboard.press('Home');
      await page.waitForTimeout(200);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(400);
      await page.screenshot({ path: join(artifacts, 'restored.png') });
      step = 'select-fetch-api';
      await page.mouse.click(810, 732);
      await page.waitForTimeout(300);
      await page.mouse.click(680, 855);
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(artifacts, 'pending-changes.png') });
      step = 'save-partial';
      await page.mouse.move(900, 850);
      await page.mouse.wheel(0, 700);
      await page.waitForTimeout(300);
      await page.mouse.click(560, 891);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-search-api' && call.args.change?.fetch === 'deepseek-fetch-api'), { timeout: 5000 });
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'partial-failure.png') });
      step = 'partial-draft-preservation';
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
      await page.mouse.click(560, 462);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-official' && call.args.change?.fetch === 'deepseek-fetch-api'), { timeout: 5000 });
    } else {
      await page.mouse.click(810, 595);
      await page.keyboard.press('ControlOrMeta+A');
      await page.keyboard.type('0');
      await page.keyboard.press('Tab');
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(artifacts, 'invalid-count.png') });
      await page.mouse.click(560, 787);
      await page.waitForTimeout(300);
      assert.equal((await page.evaluate(() => window.__verification.calls)).filter(call => call.name === 'web_settings' && call.args.change).length, 0, 'invalid numeric input never reaches IPC');
      await page.mouse.click(810, 595);
      await page.keyboard.press('ControlOrMeta+A');
      await page.keyboard.type('25');
      await page.keyboard.press('Tab');
      await page.waitForTimeout(300);
      await page.mouse.click(560, 787);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.fields?.count?.value === 25), { timeout: 5000 });
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
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-official'), { timeout: 5000 });
      await page.mouse.click(810, 301);
      await page.waitForTimeout(700);
      await page.mouse.click(680, 336);
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'search-api-restored.png') });
      step = 'save-search';
      await page.mouse.click(560, 787);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.search === 'deepseek-search-api'), { timeout: 5000 });
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
      await page.mouse.click(560, 787);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.searchBilling === 'account'), { timeout: 5000 });
      await page.mouse.click(810, 427);
      await page.waitForTimeout(700);
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(300);
      await page.keyboard.press('End');
      await page.waitForTimeout(100);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(artifacts, 'search-billing-key.png') });
      await page.mouse.click(560, 787);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.searchBilling === 'api-key'), { timeout: 5000 });
      await page.mouse.click(810, 427);
      await page.waitForTimeout(700);
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(300);
      await page.keyboard.press('Home');
      await page.waitForTimeout(100);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(500);
      await page.mouse.click(560, 787);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.searchBilling === 'auto'), { timeout: 5000 });
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
      await page.mouse.click(560, 787);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.fetch === 'deepseek-fetch-api'), { timeout: 5000 });
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(artifacts, 'fetch-billing-region.png') });
      await page.mouse.click(810, 857);
      await page.waitForTimeout(700);
      await page.screenshot({ path: join(artifacts, 'fetch-billing.png') });
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(300);
      await page.keyboard.press('Home');
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(100);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(500);
      await page.mouse.click(560, 787);
      await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings' && call.args.change?.fetchBilling === 'account'), { timeout: 5000 });
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(artifacts, 'settings-footer.png') });
      await writeFile(join(artifacts, 'settings-calls.json'), JSON.stringify(await page.evaluate(() => window.__verification.calls), null, 2));
      const changes = (await page.evaluate(() => window.__verification.calls)).filter(call => call.name === 'web_settings' && call.args.change);
      assert.equal(changes.filter(call => Object.keys(call.args.change).includes('fetchBilling')).length, 1, 'fetch billing is saved exactly once');
    }
    } catch (error) {
      await writeFile(join(artifacts, 'failed-step.json'), JSON.stringify({ step, calls: await page.evaluate(() => window.__verification.calls) }, null, 2));
      await page.screenshot({ path: join(artifacts, 'failed-step.png') });
      throw error;
    }
    await writeFile(join(artifacts, 'final-calls.json'), JSON.stringify(await page.evaluate(() => window.__verification.calls.filter(call => call.name === 'web_settings')), null, 2));
  } else if (!mode.startsWith('settings')) {
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'read_attachment'), { timeout: 20000 });
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
  for (const [themeMode, contentFontDelta, label] of mode !== 'all' ? [] : [['dark', 4, 'dark-large-font'], ['light', -2, 'light-small-font']]) {
    const themed = await browser.newPage({ viewport: { width: 1440, height: 1100 }, deviceScaleFactor: 1 });
    themed.on('pageerror', error => { errors.push(String(error)); console.error('PAGE ERROR', error); });
    themed.on('console', message => {
      if (message.type() === 'error' && message.text() !== "[ERROR] : can't render at a zero size") {
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
          case 'load_shortcuts': return { revision: 1, document: { schemaVersion: 1, profiles: {} }, error: null };
          case 'load_developer_tools': return false;
          case 'start_service': return { workspace: cwd, servicePath: '/fixture', agent: { agentInfo: { name: 'fixture' } }, configOptions: [] };
          case 'list_sessions': return { sessions: [{ sessionId, cwd, title: '公式与工具结果验收', createdAt: 1 }], nextCursor: null };
          case 'list_local_sessions': return { sessions: [] };
          case 'peek_session_titles': return { [sessionId]: '公式与工具结果验收' };
          case 'new_session': case 'resume_session': return { sessionId, cwd, configOptions: [], hostRunning: false };
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
    await themed.waitForFunction(() => document.querySelector('canvas') && !document.getElementById('boot'), { timeout: 60000 });
    await themed.waitForFunction(({ themeMode, contentFontDelta }) => window.__verification.calls.some(call =>
      call.name === 'configure_shell_window'
      && call.args.dark === (themeMode === 'dark')
    ), { themeMode }, { timeout: 20000 }).catch(async error => {
      console.error('PREFERENCE_WAIT', JSON.stringify(await themed.evaluate(() => window.__verification.calls)));
      throw error;
    });
    await themed.waitForTimeout(2000);
    await themed.screenshot({ path: join(artifacts, `${label}.png`) });
    await themed.close();
  }
  assert.deepEqual(errors, [], 'WASM and MathJax must execute without browser errors');
  console.log(JSON.stringify({ assertions: ['real WASM boot', 'history replay', 'MathJax loaded without errors', 'durable attachment loaded'], artifacts }, null, 2));
} finally {
  await browser.close(); server.close();
}
