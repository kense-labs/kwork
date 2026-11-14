import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdir, readFile } from 'node:fs/promises';
import { dirname, extname, join, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const artifacts = resolve(process.argv[2] || join(root, '.tools/verification/agent-terminal-browser'));
await mkdir(artifacts, { recursive: true });
const sessionId = 'session-agent-terminal-browser';
const terminalId = 'terminal-browser';
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
    response.writeHead(404);
    response.end(String(error));
  }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const browser = await chromium.launch({
  executablePath: process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true,
  args: ['--no-first-run', '--enable-unsafe-webgpu', '--use-angle=swiftshader'],
});
const page = await browser.newPage({ viewport: { width: 1440, height: 960 }, deviceScaleFactor: 1 });
// Canvas close control, measured in the fixture's 1440px viewport.
// This drives Chromium input, never an internal close function.
const terminalCloseTarget = { x: 951, y: 24 };
const errors = [];
page.on('pageerror', error => { errors.push(String(error)); console.error('PAGE ERROR', error); });
page.on('console', message => {
  if (message.type() === 'error' && message.text() !== "[ERROR] : can't render at a zero size") {
    errors.push(message.text());
    console.error('CONSOLE', message.text());
  }
});
const terminal = {
  id: terminalId, title: 'Agent browser', cwd, cols: 100, rows: 30,
  state: 'running', exitCode: null, origin: 'agent', control: 'agent', sandboxMode: 'workspace-write', shell: 'zsh',
};
try {
  await page.addInitScript(({ sessionId, terminal, cwd }) => {
    const listeners = [];
    const calls = [];
    let attached = false;
    let watch = 0;
    window.__verification = { calls, attached: () => attached, watch: () => watch };
    const emit = payload => {
      for (const listener of listeners) listener({ event: 'dsh-gpui-event', payload });
    };
    const frame = (type, extra = {}) => ({
      kind: type === 'state' ? 'agent_terminal_state' : 'agent_terminal_frame', sessionId, terminalId: terminal.id,
      frame: { type, ...extra },
    });
    window.__verification.emitRoster = (control = 'agent') => emit({
      kind: 'agent_terminals', sessionId, terminals: [{ ...terminal, control }],
    });
    window.__verification.emitOutput = data => emit(frame('output', { sequence: ++window.__verification.sequence, data }));
    window.__verification.emitEmpty = () => emit({ kind: 'agent_terminals', sessionId, terminals: [] });
    // The canvas does not expose AX nodes in Chromium. Physical input is
    // verified from the production IPC sequence; lifecycle assertions below
    // distinguish agent-controlled disappearance from an explicit close.
    window.__verification.sequence = 2;
    const command = async (name, args = {}) => {
      // serde_wasm_bindgen passes Maps; Tauri converts them before dispatch.
      // Preserve that production boundary in the mock rather than dropping args.
      args = JSON.parse(JSON.stringify(args, (_, value) => value instanceof Map ? Object.fromEntries(value) : value));
      calls.push({ name, args });
      switch (name) {
        case 'start_auth':
        case 'auth_status': return { apiKey: { configured: true } };
        case 'load_workspace_preferences': return { themeMode: 'light', addedWorkspaces: [cwd] };
        case 'load_shortcuts': return { revision: 1, document: { schemaVersion: 1, profiles: {} }, error: null };
        case 'load_developer_tools': return false;
        case 'start_service': return { workspace: cwd, servicePath: '/fixture', agent: { agentInfo: { name: 'fixture' } }, configOptions: [] };
        case 'list_sessions': return { sessions: [], nextCursor: null };
        case 'list_local_sessions': return { sessions: [] };
        case 'peek_session_titles': return { titles: [] };
        case 'new_session': return { sessionId, cwd, configOptions: [], hostRunning: false };
        case 'load_session_history': return { entries: [], complete: true };
        case 'turn_token_usage': return {};
        case 'permission_mode': return { mode: 'default' };
        case 'plan_mode': return { active: false };
        case 'question_state': return [];
        case 'session_commands': return { commands: [] };
        case 'inbox_state': return { items: [] };
        case 'goal_state': return { goal: null };
        case 'agent_terminal_watch':
          if (args.active === false) watch = 0;
          else if (Array.isArray(args.active)) watch = args.active.length;
          else watch = 1;
          return watch ? [{ ...terminal }] : [];
        case 'agent_terminal_attach':
          attached = true;
          return { attached: true, frame: frame('snapshot', {
            sequence: 1, screen: 'agent@workspace % printf "KWORK_BROWSER_OK\\n"\r\nKWORK_BROWSER_OK\r\nagent@workspace % ',
            info: { ...terminal }, writable: true,
          }).frame };
        case 'agent_terminal_input':
          if (args.input.type === 'write' && args.input.data === '\r' && args.input.typed) {
            queueMicrotask(() => emit(frame('state', { info: { ...terminal, control: 'user' }, writable: true })));
          }
          return { sequence: args.sequence };
        case 'agent_terminal_detach': attached = false; return null;
        case 'agent_terminal_close': return null;
        case 'agent_terminal_rename': return null;
        case 'configure_shell_window':
        case 'save_workspace_preferences':
        case 'session_read_ack':
        case 'document_system_font':
        case 'chinese_system_font': return [];
        case 'list_shells': return [];
        default: console.log(`UNHANDLED_IPC ${name} ${JSON.stringify(args)}`); return null;
      }
    };
    window.__TAURI__ = { core: { invoke: command }, event: { listen: (_name, callback) => { listeners.push(callback); return Promise.resolve(() => {}); } } };
    window.__TAURI_INTERNALS__ = { invoke: command };
  }, { sessionId, terminal, cwd });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => document.querySelector('canvas') && !document.getElementById('boot'), { timeout: 60000 });
  await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'agent_terminal_watch'), { timeout: 20000 });
  await page.evaluate(() => window.__verification.emitRoster());
  await page.waitForFunction(() => window.__verification.attached(), { timeout: 20000 });
  await page.waitForTimeout(1500);
  await page.screenshot({ path: join(artifacts, 'agent-terminal.png') });
  const calls = () => page.evaluate(() => window.__verification.calls);
  assert.equal((await calls()).filter(call => call.name === 'agent_terminal_attach').length, 1);
  await page.keyboard.type('printf "KWORK_BROWSER_INPUT\\n"\r');
  await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'agent_terminal_input' && call.args.input?.typed), { timeout: 10000 });
  await page.waitForTimeout(500);
  await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'agent_terminal_input' && call.args.input.data === '\r'));
  await page.screenshot({ path: join(artifacts, 'agent-terminal-takeover.png') });
  const input = (await calls()).find(call => call.name === 'agent_terminal_input' && call.args.input?.typed);
  assert.equal(input.args.terminalId, terminalId);
  assert.equal(input.args.sessionId, sessionId);
  const typed = (await calls()).filter(call => call.name === 'agent_terminal_input' && call.args.input?.typed);
  assert.equal(typed.at(-1).args.input.data, '\r');
  assert(typed.every(call => call.args.sequence > 0));
  assert(new Set(typed.map(call => call.args.sequence)).size === typed.length,
    'state updates must not reset the live input sequence');
  await page.evaluate(() => window.__verification.emitRoster());
  await page.waitForTimeout(300);
  assert.equal((await calls()).filter(call => call.name === 'agent_terminal_attach').length, 1, 'duplicate rosters must not reopen or reattach');
  await page.evaluate(() => window.__verification.emitRoster('user'));
  await page.waitForTimeout(300);
  assert.equal((await calls()).filter(call => call.name === 'agent_terminal_detach').length, 1,
    'a taken-over terminal leaves the Agent retain set but keeps its tab');
  await page.evaluate(() => window.__verification.emitRoster('agent'));
  await page.waitForTimeout(200);
  await page.evaluate(() => window.__verification.emitEmpty());
  await page.waitForFunction(() => !window.__verification.attached(), { timeout: 10000 });
  await page.screenshot({ path: join(artifacts, 'agent-terminal-controlled-disappeared.png') });
  // An empty Agent retain set is not a process-close request. The explicit
  // close path below is the only one allowed to terminate the shell.
  assert.equal((await calls()).filter(call => call.name === 'agent_terminal_close').length, 0);
  assert.equal((await calls()).filter(call => call.name === 'agent_terminal_detach').length, 2);
  await page.evaluate(() => window.__verification.emitRoster('user'));
  await page.waitForFunction(() => window.__verification.attached(), { timeout: 10000 });
  await page.evaluate(() => window.__verification.emitEmpty());
  await page.waitForTimeout(300);
  assert.equal((await calls()).filter(call => call.name === 'agent_terminal_detach').length, 2,
    'a taken-over terminal leaving the retain set does not re-close or re-detach its tab');
  await page.screenshot({ path: join(artifacts, 'agent-terminal-taken-over-disappeared.png') });
  await page.mouse.move(terminalCloseTarget.x, terminalCloseTarget.y);
  await page.waitForTimeout(1000);
  await page.screenshot({ path: join(artifacts, 'agent-terminal-close-hover.png') });
  await page.mouse.down();
  await page.waitForTimeout(100);
  await page.mouse.up();
  await page.waitForTimeout(1000);
  await page.screenshot({ path: join(artifacts, 'agent-terminal-close-click.png') });
  await page.waitForFunction(() => window.__verification.calls.some(call => call.name === 'agent_terminal_close'), null, { timeout: 5000 })
    .catch(async error => {
      console.error('TERMINAL_CLOSE_IPC_MISSING', JSON.stringify(await page.evaluate(() => window.__verification.calls)));
      await page.screenshot({ path: join(artifacts, 'agent-terminal-close-missing.png') });
      throw error;
    });
  assert.equal((await calls()).filter(call => call.name === 'agent_terminal_close').length, 1);
  assert.deepEqual(errors, [], 'WASM or browser runtime errors are not accepted');
  console.log(JSON.stringify({ assertions: ['real WASM boot', 'roster auto-open', 'snapshot attach', 'physical input typed=true', 'idempotent roster', 'taken-over retention', 'explicit close'], calls: (await calls()).filter(call => call.name.startsWith('agent_terminal')) }, null, 2));
} finally {
  await browser.close();
  server.close();
}
