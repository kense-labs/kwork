import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, readFile, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createInterface } from 'node:readline';

// Integration test only: run with the production Node executable, the ACP
// service checkout, and the installed rc.3 terminal runtime. Create the session
// through standard ACP. The test-only command invokes the real tool executor.
const [serviceArgument, moduleArgument, runtimeArgument] = process.argv.slice(2);
if (!serviceArgument || !moduleArgument || !runtimeArgument) {
  throw new Error('Usage: node scripts/agent-terminal-smoke.mjs <ACP service> <controller/lib/index.js> <runtime>');
}
const service = resolve(serviceArgument);
const module = resolve(moduleArgument);
const runtime = resolve(runtimeArgument);
const home = await mkdtemp(join(tmpdir(), 'kwork-agent-terminal-home-'));
const workspace = join(home, 'workspace');
await mkdir(workspace);
const source = `${await readFile(new URL('../services/session-read-status.mjs', import.meta.url), 'utf8')}
const bundledAskUser = {};
${await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8')}`
  .replace("'./agent-terminal-bridge.mjs'", JSON.stringify(new URL('../services/agent-terminal-bridge.mjs', import.meta.url).pathname))
  .replaceAll('terminalWatchMethod, terminalInputMethod,', "'_dsh/gpui/terminal-tool', terminalWatchMethod, terminalInputMethod,")
  .replace('} else if (method === terminalWatchMethod) {', `} else if (method === '_dsh/gpui/terminal-tool') {
        const { ctx } = await ready;
        const agent = liveAgent(ctx, params.sessionId);
        response = { result: await ctx.get('tools').execute({
          calls: [{ type: 'tool-call', id: params.callId, name: params.name, arguments: JSON.stringify(params.arguments) }],
          signal: new AbortController().signal,
          agent: liveAgent(ctx, params.sessionId),
        }) };
      } else if (method === terminalWatchMethod) {`)
  .replace('  return runtime;\n});', `  console.error('assertion: ACP session and terminal profile are ready');
  return runtime;
});`);
const smokeDirectory = await mkdtemp(join(tmpdir(), 'kwork-agent-terminal-smoke-'));
const smokeEntry = join(smokeDirectory, 'bridge.mjs');
await writeFile(smokeEntry, source, { mode: 0o600 });
const child = spawn(process.execPath, [smokeEntry, service,
  `${service}/apps/cli/lib/profile-boot.js`], {
    env: { ...process.env, DSH_HOME: home, KWORK_WORKSPACE: workspace,
    NODE_OPTIONS: '--disable-warning=ExperimentalWarning',
    KWORK_WORKSPACE_PREFERENCES: `${home}/preferences.json`,
    KWORK_TERMINAL_MODULE: module, KWORK_TERMINAL_RUNTIME: runtime },
  stdio: ['pipe', 'pipe', 'inherit'],
});
const childExited = once(child, 'exit');
const childClosed = childExited.then(([code, signal]) => {
  if (code || signal && signal !== 'SIGTERM') throw new Error(`ACP bridge exited: ${code ?? signal}`);
});
const stdout = createInterface({ input: child.stdout, crlfDelay: Infinity });
const pending = new Map();
const notifications = [];
let nextId = 1;
stdout.on('line', line => {
  const message = JSON.parse(line);
  if (message.id && pending.has(message.id)) {
    const request = pending.get(message.id);
    pending.delete(message.id);
    clearTimeout(request.timer);
    if (message.error) request.reject(new Error(JSON.stringify(message.error)));
    else request.resolve(message.result);
  } else {
    console.log('MESSAGE', line);
    notifications.push(message);
    if (message.method === '_dsh/gpui/terminal-frame') console.log('FRAME', JSON.stringify(message.params));
  }
});
function request(method, params = {}) {
  const id = nextId++;
  const promise = new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`Timed out: ${method}`)); }, 20000);
    pending.set(id, { resolve, reject, timer });
  });
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  return promise;
}
const stop = setTimeout(() => child.kill('SIGTERM'), 50000);
async function terminalState(predicate, description) {
  const deadline = Date.now() + 8000;
  while (Date.now() < deadline) {
    const frame = notifications.findLast(message => message.method === '_dsh/gpui/terminal-frame'
      && message.params.frame?.type === 'state' && predicate(message.params.frame));
    if (frame) return frame.params.frame;
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  throw new Error(`PTY state was not delivered: ${description}`);
}
async function rosterLength(length, sessionId, title) {
  const deadline = Date.now() + 8000;
  while (Date.now() < deadline) {
    const messages = notifications.filter(message => message.method === '_dsh/gpui/terminal-changed'
      && message.params.sessionId === sessionId);
    if (length === 0 && messages.at(-1)?.params.terminals.length === 0) return [];
    const roster = messages.findLast(message => message.params.terminals.some(terminal => terminal.title === title));
    if (roster?.params.terminals.length === length) return roster.params.terminals;
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  throw new Error(`PTY roster did not reach length ${length}`);
}
try {
  let nextCall = 1;
  async function modelTool(sessionId, name, args) {
    const batch = await request('_dsh/gpui/terminal-tool', {
      sessionId, name, arguments: args, callId: `terminal-smoke-${nextCall++}`,
    });
    assert.equal(batch.results.length, 1);
    return batch.results[0].result;
  }
  await request('initialize', { protocolVersion: 1, clientCapabilities: {} });
  const created = await request('session/new', { cwd: workspace, mcpServers: [] });
  const sessionId = created.sessionId;
  console.log('SESSION', sessionId);
  const listed = await request('_dsh/gpui/terminal-watch', { sessionId, request: 1 });
  assert.deepEqual(listed, []);
  await request('_dsh/gpui/terminal-watch', { sessionId, request: 2, active: [] });
  // Execute the model-facing plugin through the same executor used by an Agent.
  const opened = await modelTool(sessionId, 'terminal_open', {
    type: 'shell', name: 'Agent smoke', cwd: workspace,
  });
  assert.equal(opened.isError, false, JSON.stringify(opened));
  assert.equal(opened.value.status.kind, 'running');
  assert.match(opened.value.motd, /.*/);
  const terminalSessionId = opened.value.sessionId;
  const listedTools = await modelTool(sessionId, 'terminal_list', {});
  assert.equal(listedTools.isError, false, JSON.stringify(listedTools));
  assert(listedTools.value.some(terminal => terminal.sessionId === terminalSessionId));
  const roster = await rosterLength(1, sessionId, 'Agent smoke');
  const createdTerminal = roster.find(terminal => terminal.title === 'Agent smoke');
  assert(createdTerminal, 'model terminal_open did not publish its PTY to GPUI');
  const terminalId = createdTerminal.id;
  assert.match(terminalId, /^[\w-]+$/);
  assert.equal(createdTerminal.origin, 'agent');
  assert.equal(createdTerminal.control, 'agent');
  const attached = await request('_dsh/gpui/terminal-attach', { sessionId, terminalId });
  assert.equal(attached.frame.type, 'snapshot');
  assert.equal(attached.frame.writable, true);
  assert.equal(attached.frame.info.shell, 'zsh');
  assert.equal(attached.frame.info.sandboxMode, 'workspace-write');
  console.log('SNAPSHOT', attached.frame.info);
  const sent = await modelTool(sessionId, 'terminal_send', {
    sessionId: terminalSessionId, text: 'printf "KWORK_TOOL_OK\\n"',
  });
  assert.equal(sent.isError, false, JSON.stringify(sent));
  assert.match(sent.value.viewport, /KWORK_TOOL_OK/);
  const read = await modelTool(sessionId, 'terminal_read', { sessionId: terminalSessionId, count: 100 });
  assert.equal(read.isError, false, JSON.stringify(read));
  assert.match(read.value.text, /KWORK_TOOL_OK/);
  const background = await modelTool(sessionId, 'terminal_send', {
    sessionId: terminalSessionId, text: 'printf "KWORK_BACKGROUND_OK\\n"',
    run_in_background: true,
  });
  assert.equal(background.isError, false, JSON.stringify(background));
  const jobId = background.value?.jobId;
  assert(jobId, `background terminal_send must return a job id: ${JSON.stringify(background)}`);
  const jobOutput = await modelTool(sessionId, 'job_output', {
    job_id: jobId, wait: true, timeout_ms: 10000,
  });
  assert.equal(jobOutput.isError, false, JSON.stringify(jobOutput));
  assert.match(jobOutput.value.text, /KWORK_BACKGROUND_OK/);
  assert.equal(jobOutput.value.job.status, 'completed');
  const jobKill = await modelTool(sessionId, 'job_kill', { job_id: jobId });
  assert.equal(jobKill.isError, false, JSON.stringify(jobKill));
  assert.equal(jobKill.value.outcome, 'already-finished');
  console.log('assertion: background terminal_send, job_output and job_kill passed');
  await request('_dsh/gpui/terminal-input', { sessionId, terminalId, sequence: 1,
    type: 'write', data: 'printf "KWORK_AGENT_OK\\n"\n', typed: true });
  const deadline = Date.now() + 8000;
  while (Date.now() < deadline && !notifications.some(message =>
    message.method === '_dsh/gpui/terminal-frame' && message.params.frame?.data?.includes('KWORK_AGENT_OK')
  )) await new Promise(resolve => setTimeout(resolve, 25));
  assert(notifications.some(message => message.method === '_dsh/gpui/terminal-frame'
    && message.params.frame?.data?.includes('KWORK_AGENT_OK')), 'PTY output was not delivered');
  await terminalState(frame => frame.info.control === 'user' && frame.writable, 'user takeover');
  const refused = await modelTool(sessionId, 'terminal_send', {
    sessionId: terminalSessionId, text: 'printf "MUST_NOT_RUN\\n"',
  });
  assert.equal(refused.isError, true, 'user takeover must refuse model input');
  assert.match(refused.error.message, /taken over/);
  const readableAfterTakeover = await modelTool(sessionId, 'terminal_read', { sessionId: terminalSessionId });
  assert.equal(readableAfterTakeover.isError, false, JSON.stringify(readableAfterTakeover));
  assert.match(readableAfterTakeover.value.text, /KWORK_AGENT_OK/);
  const signalled = await modelTool(sessionId, 'terminal_signal', { sessionId: terminalSessionId, signal: 'SIGTSTP' });
  assert.equal(signalled.isError, true, 'user takeover must also refuse model signals');
  await request('_dsh/gpui/terminal-input', { sessionId, terminalId, sequence: 2,
    type: 'resize', cols: 100, rows: 40 });
  await terminalState(frame => frame.info.cols === 100 && frame.info.rows === 40, '100x40 resize');
  await request('_dsh/gpui/terminal-rename', { sessionId, terminalId, title: 'Smoke terminal' });
  await terminalState(frame => frame.info.title === 'Smoke terminal', 'rename');
  await request('_dsh/gpui/terminal-detach', { sessionId, terminalId });
  const retained = await request('_dsh/gpui/terminal-watch', { sessionId, request: 4, active: [terminalId] });
  assert.equal(retained[0].id, terminalId);
  assert.equal(retained[0].title, 'Smoke terminal');
  const reattached = await request('_dsh/gpui/terminal-attach', { sessionId, terminalId });
  assert(reattached.frame.screen.includes('KWORK_AGENT_OK'), 'retained PTY screen was lost');
  await request('_dsh/gpui/terminal-detach', { sessionId, terminalId });
  const closedByTool = await modelTool(sessionId, 'terminal_close', { sessionId: terminalSessionId });
  assert.equal(closedByTool.isError, false, JSON.stringify(closedByTool));
  assert.equal(closedByTool.value.outcome, 'closed');
  const stillOwned = await request('_dsh/gpui/terminal-watch', { sessionId, request: 3 });
  assert.equal(stillOwned[0].id, terminalId, 'releasing a taken-over model terminal must not kill the user shell');
  await request('_dsh/gpui/terminal-close', { sessionId, terminalId });
  await rosterLength(0, sessionId, 'Smoke terminal');
  const closed = await request('_dsh/gpui/terminal-watch', { sessionId, request: 3 });
  assert.deepEqual(closed, []);
  console.log('assertion: actual ACP PTY snapshot, input, output, takeover, resize, rename, detach, retain and close passed');
} finally {
  clearTimeout(stop);
  child.kill('SIGTERM');
  await childExited;
  await rm(smokeDirectory, { recursive: true, force: true });
  await rm(home, { recursive: true, force: true });
}
