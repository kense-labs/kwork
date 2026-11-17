/** Actual Electron Platform lifecycle; all credentials and endpoints are fixtures. */
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(new URL('../package.json', import.meta.url)));
const helper = join(root, 'services/account-platform-electron');
const executable = process.env.KWORK_ELECTRON_TEST_EXECUTABLE || join(helper, 'node_modules/electron/dist',
  process.platform === 'darwin' ? 'Electron.app/Contents/MacOS/Electron' : process.platform === 'win32' ? 'electron.exe' : 'electron');
const artifacts = await mkdtemp(join(tmpdir(), 'kwork-electron-platform-'));
const requests = [], reports = [], locales = [];
let child;
async function waitFor(predicate, message) {
  const until = Date.now() + 20000;
  while (Date.now() < until) { if (predicate()) return; await new Promise(resolve => setTimeout(resolve, 50)); }
  throw new Error(message);
}
const listen = server => new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const foreign = createServer((request, response) => {
  requests.push({ path: '/foreign', headers: request.headers });
  response.writeHead(200, { 'Content-Type': 'image/svg+xml' });
  response.end('<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>');
});
await listen(foreign);
const server = createServer(async (request, response) => {
  const path = new URL(request.url, 'http://localhost').pathname;
  requests.push({ path, headers: request.headers });
  if (path === '/report' || path === '/locale') {
    let body = ''; for await (const chunk of request) body += chunk;
    (path === '/report' ? reports : locales).push(JSON.parse(body));
    response.end('ok'); return;
  }
  if (path === '/usage' || path === '/top_up') {
    response.writeHead(200, { 'Content-Type': 'text/html', 'Set-Cookie': ['ordinary=keep; Path=/', 'route=server; Path=/'] });
    response.end(`<!doctype html><script>
      const stored = localStorage.getItem('preference'); localStorage.setItem('preference', 'retained');
      const bootstrap = { token: window.dsh.getAuthToken(), locale: window.dsh.getLocale(), mode: window.dsh.displayMode, version: window.dsh.protocolVersion };
      window.dsh.onLocaleChange(locale => fetch('/locale', {method:'POST',body:JSON.stringify({locale,current:window.dsh.getLocale()})}));
      const worker = new Worker('/worker-start');
      const completed = new Promise(resolve => worker.onmessage = resolve);
      const image = new Image(); const redirected = new Promise(resolve => { image.onload = resolve; image.onerror = resolve; }); image.src='/asset-start';
      const frame = document.createElement('iframe'); frame.src='/frame'; document.addEventListener('DOMContentLoaded', () => document.body.append(frame));
      Promise.all([fetch('/verified'), completed, redirected]).then(() => fetch('/report', {method:'POST',body:JSON.stringify({bootstrap,stored,path:location.pathname})}));
    </script><body>Platform fixture</body>`); return;
  }
  if (path === '/worker-start') { response.writeHead(302, { Location: '/worker.js' }); response.end(); return; }
  if (path === '/worker.js') { response.writeHead(200, { 'Content-Type': 'text/javascript' }); response.end("fetch('/worker-verified').then(()=>postMessage('done'))"); return; }
  if (path === '/asset-start') { response.writeHead(302, { Location: '/asset-second' }); response.end(); return; }
  if (path === '/asset-second') { response.writeHead(302, { Location: `http://127.0.0.1:${foreign.address().port}/foreign` }); response.end(); return; }
  if (path === '/frame') { response.writeHead(200, { 'Content-Type': 'text/html' }); response.end("<script>fetch('/frame-verified?bridge='+String(typeof window.dsh))</script>"); return; }
  response.writeHead(200, { 'Content-Type': 'text/plain' }); response.end('ok');
});
await listen(server);
const origin = `http://127.0.0.1:${server.address().port}`;
const profile = join(artifacts, 'profile');

async function start() {
  const environment = { ...process.env }; delete environment.ELECTRON_RUN_AS_NODE;
  child = spawn(executable, [join(helper, 'dist'), `--kwork-platform-data=${profile}`, '--kwork-client-version=0.1.0', '--enable-logging=stderr'],
    { env: environment, stdio: ['pipe', 'pipe', 'pipe'] });
  const messages = [], errors = [];
  child.stderr.on('data', data => errors.push(data.toString()));
  createInterface({ input: child.stdout }).on('line', line => {
    try { messages.push(JSON.parse(line)); } catch { errors.push(line); }
  });
  child.on('error', error => errors.push(String(error)));
  const details = () => JSON.stringify({ exitCode: child.exitCode, signal: child.signalCode, messages, errors });
  await waitFor(() => messages.some(message => message.event === 'ready') || child.exitCode !== null, 'helper startup timeout');
  assert.ok(messages.some(message => message.event === 'ready'), `helper must start: ${details()}`);
  return async request => {
    child.stdin.write(JSON.stringify(request) + '\n');
    await waitFor(() => messages.some(message => message.id === request.id && 'ok' in message) || child.exitCode !== null, `request ${request.action} timed out`);
    assert.equal(messages.find(message => message.id === request.id && 'ok' in message)?.ok, true, `${request.action} succeeds: ${details()}`);
    if (request.action === 'close') {
      await waitFor(() => child.exitCode !== null, 'helper exits after cleanup'); assert.equal(child.exitCode, 0);
    }
  };
}

try {
  for (const [index, page] of ['usage', 'top-up'].entries()) {
    const command = await start();
    await command({ id: 1, action: 'open', page, width: 600, height: 400, locale: 'zh_CN', account: {
      origin, token: 'fixture-token', userId: 'fixture-user', embeddedPageDist: 'preview',
      requestHeaders: { cookie: 'route=preview; deployment=active', 'x-deployment-test': 'owned' },
    } });
    await waitFor(() => reports.length === index + 1, 'page completes fetch, Worker, and redirected asset');
    assert.deepEqual(reports[index].bootstrap, { token: 'fixture-token', locale: 'zh_CN', mode: 'embedded', version: 1 });
    assert.equal(reports[index].stored, index === 0 ? null : 'retained', 'account preferences survive process restart');
    await command({ id: 2, action: 'locale', locale: 'en_US' });
    await waitFor(() => locales.length === index + 1, 'locale callback fires');
    assert.deepEqual(locales[index], { locale: 'en_US', current: 'en_US' });
    await command({ id: 3, action: 'close' });
  }
  for (const path of ['/verified', '/worker.js', '/worker-verified', '/asset-second']) {
    const matches = requests.filter(request => request.path === path);
    assert.equal(matches.length, 2, `${path} executed on both visits`);
    for (const { headers } of matches) {
      assert.match(headers.cookie, /ordinary=keep/);
      assert.match(headers.cookie, /route=preview/); assert.doesNotMatch(headers.cookie, /route=server/);
      assert.match(headers.cookie, /deployment=active/);
      assert.equal(headers['x-client-platform'], process.platform === 'win32' ? 'desktop-win' : 'desktop-mac');
      assert.equal(headers['x-deployment-test'], 'owned');
    }
  }
  for (const { headers } of requests.filter(request => request.path === '/foreign')) {
    assert.equal(headers.cookie, undefined); assert.equal(headers['x-deployment-test'], undefined);
    assert.equal(headers['x-client-version'], undefined);
  }
  assert.equal(requests.filter(request => request.path === '/foreign').length, 2);
  for (const request of requests.filter(request => ['/usage', '/top_up'].includes(request.path))) {
    assert.doesNotMatch(request.headers.cookie, /ordinary=keep/, 'authentication cookies cleared before each visit');
  }
  assert.equal(requests.filter(request => request.path === '/frame-verified').length, 2);
  await writeFile(join(artifacts, 'report.json'), JSON.stringify({ reports, locales, requests }, null, 2));
  console.log(JSON.stringify({ ok: true, platform: process.platform, artifacts, assertions: ['real upstream Electron preload', 'cookie merge after server cookies', 'Worker and redirected assets', 'foreign-origin header stripping', 'locale notification', 'LocalStorage across helper restart', 'authentication cleanup'] }, null, 2));
} finally {
  if (child && child.exitCode === null) child.kill();
  await Promise.all([new Promise(resolve => server.close(resolve)), new Promise(resolve => foreign.close(resolve))]);
}
