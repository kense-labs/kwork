import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, extname, join, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inflateSync } from 'node:zlib';
import { chromium } from 'playwright-core';

const root = dirname(fileURLToPath(new URL('../package.json', import.meta.url)));
process.chdir(root);
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
    const body = await readFile(path);
    response.writeHead(200, { 'Content-Type': mime[extname(path)] || 'application/octet-stream' });
    response.end(body);
  } catch (error) {
    if (response.headersSent) response.destroy(error);
    else { response.writeHead(404); response.end(String(error)); }
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
  if (message.type() === 'error') {
    errors.push(message.text()); console.error('CONSOLE', message.text());
  }
  if (message.text().startsWith('web settings')) console.log(message.text());
});
const png = (await readFile(join(root, 'src-tauri/icons/64x64.png'))).toString('base64');
const imageAttachment = { attachmentId: 'a'.repeat(64), mediaType: 'image/png', bytes: Buffer.from(png, 'base64').length, width: 64, height: 64, name: 'fixture.png' };
assert.ok(['all', 'image', 'inherited', 'theme', 'stream', 'trajectory', 'trajectory-long', 'trajectory-narrow', 'settings', 'settings-partial', 'settings-readonly', 'settings-search', 'settings-fetch', 'appearance', 'plugins', 'workflow', 'team'].includes(mode), `unknown mode: ${mode}`);
const turnStart = { type: 'turn/start', seq: 1, data: { turn: 1 } };
const userMessage = { type: 'user/message', seq: 2, surfaceOp: 'append', data: { id: 'user', source: { kind: 'user' }, role: 'user', content: [{ type: 'text', text: '请验证公式、差异和图片' }] } };
const turnEnd = { type: 'turn/end', seq: 10, data: { turn: 1, reason: { kind: 'completed' } } };
const longTrajectoryEvents = Array.from({ length: 30 }, (_, index) => {
  const seq = index + 1;
  const time = 1000 + index * 200;
  return { type: 'tool/call', seq, time, data: { turn: 1, step: 1, callId: `long-${seq}`, name: 'bash', arguments: JSON.stringify({ command: `step-${String(seq).padStart(2, '0')}` }) } };
});
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
const events = mode === 'inherited'
  ? [
      { type: 'user/message', seq: 1, data: { id: 'parent', content: [{ type: 'text', text: '继承的父会话消息' }] } },
      { type: 'assistant/message', seq: 2, data: { turn: 1, step: 1, message: { id: 'parent-reasoning', content: [{ type: 'reasoning', text: '继承的思考' }] } } },
      { type: 'session/end-seed', seq: 3, data: { inherited: true } },
      { type: 'assistant/message', seq: 4, data: { turn: 1, step: 1, message: { id: 'child-answer', content: [{ type: 'text', text: '分叉后的新回复' }] } } },
      { type: 'turn/end', seq: 5, data: { turn: 1, reason: { kind: 'forked' } } },
    ]
  : mode === 'image'
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
  : mode === 'trajectory-long'
    ? [
      { type: 'turn/start', seq: 1, time: 800, data: { turn: 1 } },
      { type: 'user/message', seq: 2, time: 900, data: { id: 'long-user', source: { kind: 'user' }, content: [{ type: 'text', text: 'Long trajectory' }] } },
      { type: 'step/start', seq: 3, time: 950, data: { turn: 1, step: 1 } },
      ...longTrajectoryEvents.map(event => ({ ...event, seq: event.seq + 3 })),
    ]
  : mode === 'trajectory' || mode === 'trajectory-narrow'
    ? [
      { type: 'turn/start', seq: 1, time: 1000, data: { turn: 1 } },
      { type: 'user/message', seq: 2, time: 1100, data: { id: 'u', source: { kind: 'user' }, content: [{ type: 'text', text: 'Inspect this' }] } },
      { type: 'step/start', seq: 3, time: 1200, data: { turn: 1, step: 1 } },
      { type: 'assistant/message', seq: 4, time: 1600, data: { turn: 1, step: 1, message: { id: 'a', source: { provider: 'deepseek', model: 'v4' }, content: [{ type: 'reasoning', text: 'Think' }, { type: 'text', text: 'Done' }] }, usage: { inputTokens: 20, outputTokens: 5 } } },
      { type: 'tool/call', seq: 5, time: 1700, data: { turn: 1, step: 1, callId: 'c', name: 'bash', arguments: '{"command":"pwd"}' } },
      { type: 'tool/result', seq: 6, time: 1900, data: { turn: 1, step: 1, message: { source: { callId: 'c' }, content: [{ type: 'text', text: '/tmp' }] } } },
      { type: 'step/end', seq: 7, time: 2000, data: { turn: 1, step: 1 } },
      { type: 'step/start', seq: 8, time: 2100, data: { turn: 1, step: 2 } },
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
    window.__verification = { calls, events: [], emit: payload => listeners.forEach(listener => listener({ event: 'dsh-gpui-event', payload })) };
    for (const phase of ['pointerdown', 'pointerup', 'click']) {
      window.addEventListener(phase, event => window.__verification.events.push({ phase, x: event.clientX, y: event.clientY, hit: document.elementFromPoint(event.clientX, event.clientY)?.tagName }), true);
    }
    window.__TAURI__ = {
      core: { invoke: async (name, args = {}) => {
        args = JSON.parse(JSON.stringify(args, (_, value) => value instanceof Map ? Object.fromEntries(value) : value));
        calls.push({ name, args });
        switch (name) {
          case 'start_auth': case 'auth_status': return { apiKey: { configured: true } };
          case 'load_workspace_preferences': return { themeMode: mode === 'appearance' ? 'light' : 'dark', addedWorkspaces: [cwd], workDetails: 'verbose', contentFontDelta: 0, scratchSession: sessionId };
          case 'load_shortcuts': return { revision: 1, document: { schemaVersion: 1, profiles: {} }, error: null };
          case 'load_developer_tools': return false;
          case 'start_service': return { workspace: cwd, servicePath: '/fixture', agent: { agentInfo: { name: 'fixture' } }, configOptions: [{ id: 'model', currentValue: 'fixture-model' }] };
          case 'list_sessions': return { sessions: [{ sessionId, cwd, title: '公式与工具结果验收', createdAt: 1 }], nextCursor: null };
          case 'list_local_sessions': return { sessions: [] };
          case 'peek_session_titles': return { [sessionId]: '公式与工具结果验收' };
          case 'new_session': case 'resume_session': return { sessionId, cwd, configOptions: [{ id: 'model', currentValue: 'fixture-model' }], hostRunning: false };
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
  await page.waitForFunction(() => document.querySelector('canvas') && !document.getElementById('boot'), null, { timeout: 60000 });
  await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'load_session_history'), null, { timeout: 20000 }).catch(async error => {
    console.error('STARTUP_CALLS', JSON.stringify(await page.evaluate(() => window.__verification.calls)));
    await page.screenshot({ path: join(artifacts, 'startup-failed.png') });
    throw error;
  });
  await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'turn_token_usage'), null, { timeout: 20000 });
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
    await page.mouse.click(1120, 1039);
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(artifacts, 'menu.png') });
    await page.mouse.click(1080, 995);
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(artifacts, 'pane.png') });
    await page.mouse.click(1158, 952);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'team_state' && call.args.enabled === true), null, { timeout: 5000 });
    await page.waitForTimeout(500);
    await page.screenshot({ path: join(artifacts, 'enabled.png') });
  } else if (mode === 'plugins') {
    await page.mouse.click(50, 168);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'plugin_center'), null, { timeout: 10000 });
    await page.waitForTimeout(1500);
    await page.screenshot({ path: join(artifacts, 'plugin-badges.png') });
    await page.mouse.click(560, 305);
    await page.waitForTimeout(700);
    await page.screenshot({ path: join(artifacts, 'plugin-detail.png') });
  } else if (mode === 'trajectory-long') {
    await page.mouse.click(1318, 25);
    await page.waitForTimeout(900);
    // Read the rendered blue status dots. The Canvas has no DOM or native
    // accessibility nodes; no private Rust layout mirror is used here.
    const rows = async name => {
      const screenshot = await page.screenshot({ path: join(artifacts, `${name}.png`) });
      const png = await page.evaluate(source => new Promise((resolve, reject) => {
        const image = new Image();
        image.onload = () => {
          const canvas = document.createElement('canvas');
          canvas.width = image.width;
          canvas.height = image.height;
          const context = canvas.getContext('2d');
          context.drawImage(image, 0, 0);
          const { data } = context.getImageData(1410, 158, 22, 820);
          const centers = [];
          for (let y = 1; y < 819; y++) {
            let matches = 0;
            for (let x = 1; x < 21; x++) {
              const offset = (y * 22 + x) * 4;
              const [red, green, blue, alpha] = data.slice(offset, offset + 4);
              if (alpha > 250 && blue * 255 / alpha > 180 && (blue - red) * 255 / alpha > 50 && (green - red) * 255 / alpha > 25) matches++;
            }
            if (matches >= 10) centers.push(y + 158);
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
      }), `data:image/png;base64,${screenshot.toString('base64')}`);
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
  } else if (mode === 'trajectory' || mode === 'trajectory-narrow') {
    await page.waitForTimeout(500);
    assert.equal((await page.evaluate(() => window.__verification.calls)).filter(call => ['new_session', 'prompt', 'queue_prompt'].includes(call.name)).length, 0, 'history remains read-only');
    if (mode === 'trajectory-narrow') {
      await page.setViewportSize({ width: 620, height: 820 });
      await page.waitForTimeout(500);
      await page.mouse.click(492, 25);
    } else {
      await page.mouse.click(1318, 25);
    }
    await page.waitForTimeout(700);
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
        const distances = [left, above, upperLeft].map(value => Math.abs(estimate - value));
        return [left, above, upperLeft][distances.indexOf(Math.min(...distances))];
      };
      for (let y = 0; y < height; y++) {
        const row = y * stride;
        pixels[row] = filtered[row];
        for (let x = 1; x < stride; x++) {
          const left = x > 3 ? pixels[row + x - 3] : 0;
          const above = y ? pixels[row - stride + x] : 0;
          const upperLeft = y && x > 3 ? pixels[row - stride + x - 3] : 0;
          const predictor = [0, left, above, Math.floor((left + above) / 2), paeth(left, above, upperLeft)][filtered[row]];
          assert.notEqual(predictor, undefined, `unsupported PNG row filter ${filtered[row]}`);
          pixels[row + x] = (filtered[row + x] + predictor) & 255;
        }
      }
      const channel = (x, y, component) => pixels[y * stride + 1 + x * 3 + component];
      const edges = [];
      const firstRecordY = 157;
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
      const tagBackground = [0, 1, 2].map(component => channel(343, 198, component));
      for (let y = 194; y < 211; y++) {
        for (let tagX = narrow ? 64 : 340; tagX < (narrow ? 83 : 369); tagX++) {
          const contrast = [0, 1, 2].map(component => Math.abs(channel(tagX, y, component) - tagBackground[component]));
          const blueIcon = channel(tagX, y, 2) - channel(tagX, y, 0) >= 100;
          if ((narrow && blueIcon) || (!narrow && Math.max(...contrast) >= 50)) tagInk.push([tagX, y]);
        }
      }
      const boundaryX = mode === 'trajectory-narrow' ? 294 : 300;
      const boundaryInk = [];
      for (let y = 151; y < Math.min(height, 400); y++) {
        for (let x = boundaryX + 7; x <= boundaryX + 9; x++) {
          const rgb = [0, 1, 2].map(component => channel(x, y, component));
          if (rgb.every(value => value >= 30 && value <= 170)) boundaryInk.push([x, y]);
        }
      }
      return { edges: edges.slice(0, 12), boundaryInk, tagInk, rowTransition, channel };
    };
    const expanded = await layout('trajectory');
    if (mode === 'trajectory-narrow') {
      assert.deepEqual(expanded.edges.slice(0, 3), [186, 216, 246], 'narrow history has three 30px content rows without a duplicate assistant row');
      assert.ok(expanded.tagInk.length > 0 && expanded.tagInk.every(([x]) => x >= 64 && x <= 82), `narrow type label leaves the 19px icon slot: ${JSON.stringify(expanded.tagInk)}`);
      assert.ok(expanded.boundaryInk.some(([, y]) => y >= 247 && y <= 257), `narrow terminal boundary dot stays inside its 9px seat: ${JSON.stringify(expanded.boundaryInk)}`);
      await page.mouse.click(300, 201);
      await page.waitForTimeout(300);
      const requestInspector = await layout('request-inspector');
      const inspectorTransition = layout => Math.max(...Array.from({ length: 120 }, (_, index) => index + 260).map(y =>
        Math.max(...[0, 1, 2].map(component => Math.abs(layout.channel(291, y, component) - layout.channel(292, y, component))))
      ));
      assert.ok(inspectorTransition(requestInspector) >= 8, 'narrow record opens the official right-side inspector drawer');
      await page.keyboard.press('Escape');
      await page.waitForTimeout(300);
      const closedInspector = await layout('request-inspector-closed');
      assert.ok(inspectorTransition(closedInspector) < 8, 'Escape closes the narrow inspector drawer');
      await page.mouse.click(28, 255);
      await page.waitForTimeout(300);
      const boundaryInspector = await layout('request-boundary-inspector');
      assert.ok(inspectorTransition(boundaryInspector) >= 8, 'the trailing request boundary opens its request details');
      await writeFile(join(artifacts, 'report.json'), JSON.stringify({ mode, ...expanded, errors }, null, 2));
    } else {
      assert.deepEqual(expanded.edges.slice(0, 3), [186, 216, 246], 'request metadata no longer duplicates assistant content');
      assert.ok(expanded.tagInk.length > 0 && expanded.tagInk.some(([x]) => x >= 359), `wide assistant kind tag shows its text label: ${JSON.stringify(expanded.tagInk)}`);
      assert.ok(expanded.boundaryInk.some(([, y]) => y >= 247 && y <= 257), `terminal boundary dot stays inside its 9px seat: ${JSON.stringify(expanded.boundaryInk)}`);
      const beforeRequest = await page.screenshot();
      await page.mouse.click(300, 250);
      await page.waitForTimeout(300);
      const requestInspector = await page.screenshot({ path: join(artifacts, 'request-inspector.png') });
      assert.notDeepEqual(requestInspector, beforeRequest, 'terminal request opens a real inspector');
      await page.keyboard.press('Escape');
      await page.waitForTimeout(300);
      await page.mouse.click(600, 171, { clickCount: 2 });
      await page.waitForTimeout(300);
      const folded = await layout('folded');
      assert.ok(folded.rowTransition(186) >= 8 && folded.rowTransition(206) >= 8, 'the fold preserves the first 30px record and adds one 20px summary');
      assert.ok(folded.rowTransition(216) < 8, 'the folded assistant and tool records no longer occupy content rows');
      await page.keyboard.press('Tab');
      await page.waitForTimeout(100);
      await page.screenshot({ path: join(artifacts, 'summary-focused.png') });
      await page.keyboard.press('Enter');
      await page.waitForTimeout(300);
      const enterExpanded = await layout('keyboard-enter');
      const contentEdges = layout => layout.edges.filter(y => y >= 186 && !layout.edges.includes(y - 2)).slice(0, 3);
      assert.deepEqual(contentEdges(enterExpanded), [186, 216, 246], 'Enter expands the focused summary');
      await page.mouse.click(600, 197, { clickCount: 2 });
      await page.waitForTimeout(200);
      await page.keyboard.press('Tab');
      await page.waitForTimeout(100);
      await page.keyboard.press('Space');
      await page.waitForTimeout(300);
      const spaceExpanded = await layout('keyboard-space');
      assert.deepEqual(contentEdges(spaceExpanded), [186, 216, 246], 'Space expands the focused summary');
      await writeFile(join(artifacts, 'report.json'), JSON.stringify({ mode, expanded, folded, errors }, null, 2));
    }
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
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'save_workspace_preferences' && call.args.preferences.themeMode === 'dark' && call.args.preferences.contentFontDelta === 4), null, { timeout: 5000 });
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
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'web_settings'), null, { timeout: 20000 });
    await page.waitForTimeout(1500);
    await page.screenshot({ path: join(artifacts, 'web-search.png') });
    const settingsCalls = await page.evaluate(() => window.__verification.calls);
    await writeFile(join(artifacts, 'settings-calls.json'), JSON.stringify(settingsCalls, null, 2));
    assert.equal(settingsCalls.filter(call => call.name === 'web_settings').length, 1, 'settings navigation requests the snapshot once');
    await page.mouse.click(430, 322);
    await page.waitForTimeout(300);
    await page.mouse.click(662, 298);
    await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'save_workspace_preferences' && call.args.preferences.themeMode === 'dark' && call.args.preferences.contentFontDelta === 0), null, { timeout: 5000 });
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
      await page.mouse.click(680, 550);
      await page.waitForTimeout(500);
      await page.waitForTimeout(100);
      await page.keyboard.press('Enter');
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
  } else if (!mode.startsWith('settings')) {
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
  for (const [themeMode, contentFontDelta, label] of mode === 'inherited' ? [['light', 0, 'inherited-light']] : mode !== 'all' ? [] : [['dark', 4, 'dark-large-font'], ['light', -2, 'light-small-font']]) {
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
  console.log(JSON.stringify({ assertions: ['real WASM boot', 'history replay', 'MathJax loaded without errors', 'durable attachment loaded'], artifacts }, null, 2));
} finally {
  await browser.close(); server.close();
}
