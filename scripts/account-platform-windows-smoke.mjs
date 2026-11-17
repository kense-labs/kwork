import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { once } from 'node:events';
import { access, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { promisify } from 'node:util';
import { chromium } from 'playwright-core';
import { stringify } from 'yaml';

const hostVersion = '0.2.2-rc.3';
const token = 'synthetic-kwork-platform-grant';
const routeCookie = 'routing=deployment';
const existingCookie = 'ordinary=server';

async function serve(handle) {
  const server = createServer(handle);
  const listening = once(server, 'listening');
  server.listen(0, '127.0.0.1');
  await listening;
  return {
    origin: 'http://127.0.0.1:' + server.address().port,
    close: async () => {
      const closed = once(server, 'close');
      server.close();
      server.closeAllConnections();
      await closed;
    },
  };
}

async function installHost(root) {
  const host = join(root, 'host');
  const entry = join(host, 'apps', 'cli', 'lib');
  await mkdir(entry, { recursive: true });
  await writeFile(join(host, 'package.json'), JSON.stringify({
    private: true, type: 'module', dependencies: { '@deepseek-ai/dsh': hostVersion },
  }));
  const installer = spawn(process.execPath, ['install', '--cwd', host, '--linker', 'hoisted', '--ignore-scripts'], {
    env: { ...process.env, BUN_INSTALL_CACHE_DIR: join(root, 'install-cache') },
    stdio: 'inherit',
  });
  const [code] = await once(installer, 'exit');
  assert.equal(code, 0, 'Install the pinned official ACP package');
  await writeFile(join(entry, '..', 'package.json'), '{"type":"module"}');
  await writeFile(join(entry, 'bin.js'), 'import "../../../node_modules/@deepseek-ai/dsh/lib/bin.js";\n');
  await writeFile(join(entry, 'profile-boot.js'),
    'export { runProfile } from "../../../node_modules/@deepseek-ai/dsh/lib/profile-boot.js";\n');
  return host;
}

export async function platformFixture(home) {
  const records = [];
  const record = (request, path, url) => {
    const cookies = (request.headers.cookie ?? '').split(/;\s*/);
    const entry = {
      path,
      route: cookies.includes(routeCookie),
      ordinary: cookies.includes(existingCookie),
      deployment: request.headers['x-platform-preview'] === 'fixture',
      auth: request.headers['x-dsh-auth-token'] === token,
      client: request.headers['x-client-platform'],
      pref: url.searchParams.get('pref'),
    };
    records.push(entry);
    return entry;
  };
  const outside = await serve((request, response) => {
    const url = new URL(request.url, 'http://fixture.invalid');
    record(request, 'outside:' + url.pathname, url);
    response.writeHead(200, { 'content-type': 'application/javascript' }).end('');
  });
  let mode = 'blocked';
  let balance = '7.50';
  let platform;
  try {
    platform = await serve((request, response) => {
      const url = new URL(request.url, 'http://fixture.invalid');
      const path = url.pathname;
      const json = (body) => response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(body));
      const business = data => json({ code: 0, data: { biz_code: 0, biz_data: data } });
      const html = body => response.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' }).end(body);
      if (path === '/usage' || path === '/top_up') {
        record(request, path + ':' + mode, url);
        if (mode === 'blocked') {
          response.writeHead(307, { location: outside.origin + '/wrong-document' }).end();
        } else if (mode === 'disconnect') {
          request.socket.destroy();
        } else {
          response.writeHead(307, { location: platform.origin + '/page' + path }).end();
        }
      } else if (path === '/page/usage' || path === '/page/top_up') {
        record(request, path, url);
        html(`<!doctype html><title>KWork test platform</title>
          <script src="/static-asset"></script>
          <script type="module">
            const pref = localStorage.getItem('kwork-platform-smoke') ?? 'empty';
            const headers = { 'x-dsh-auth-token': await dsh.getAuthToken() };
            const cookie = await fetch('/session-cookie', { headers });
            if (!cookie.ok) throw Error('Platform session rejected');
            await new Promise((resolve, reject) => {
              const script = document.createElement('script');
              script.onload = resolve;
              script.onerror = reject;
              script.src = '/redirected-asset';
              document.head.append(script);
            });
            const worker = new Worker('/fixture-worker.js');
            await new Promise((resolve, reject) => {
              worker.onmessage = resolve;
              worker.onerror = reject;
            });
            worker.terminate();
            localStorage.setItem('kwork-platform-smoke', 'saved');
            await fetch('/verified?pref=' + encodeURIComponent(pref), { headers });
          </script>`);
      } else if (path === '/session-cookie' || path === '/verified') {
        const received = record(request, path, url);
        if (!received.auth) response.writeHead(401).end();
        else if (path === '/session-cookie') {
          response.writeHead(200, { 'set-cookie': existingCookie + '; Path=/; HttpOnly; Max-Age=7776000' }).end('');
        } else response.writeHead(200).end('');
      } else if (path === '/redirected-asset') {
        record(request, path, url);
        response.writeHead(302, { location: outside.origin + '/redirected-asset' }).end();
      } else if (path === '/fixture-worker.js') {
        record(request, path, url);
        response.writeHead(200, { 'content-type': 'application/javascript' })
          .end("fetch('/worker-fetch').then(() => postMessage('done'));");
      } else if (path === '/worker-fetch' || path === '/static-asset') {
        record(request, path, url);
        response.writeHead(200, { 'content-type': 'application/javascript' }).end('');
      } else if (request.headers['x-dsh-auth-token'] !== token) {
        response.writeHead(401).end();
      } else if (path.endsWith('/models')) {
        json({ data: [{ id: 'deepseek-v4-pro', api_capabilities: { anthropic_messages: {} } }] });
      } else if (path.endsWith('/current')) {
        business({ id: 'fixture-owner', email: 'fi***@example.invalid', id_profile: { name: 'Platform Fixture' } });
      } else if (path === '/api/v0/users/notifications/list') {
        business({ next_poll_after: 60, notifications: [] });
      } else if (path.endsWith('/get_unnotified_bonuses')) {
        business([]);
      } else {
        business({ normal_wallets: [{ currency: 'CNY', balance }], bonus_wallets: [] });
      }
    });
    const profile = join(home, 'profiles', 'acp');
    await mkdir(profile, { recursive: true });
    await writeFile(join(profile, 'cordis.patch.yml'), stringify([
      { id: 'deepseek-account', config: {
        platformOrigin: platform.origin, harnessOrigin: platform.origin, inferenceOrigin: platform.origin,
        allowLoopbackHttp: true,
        desktopPlatform: 'win32', requestHeaders: { Cookie: routeCookie, 'X-Platform-Preview': 'fixture' },
      } },
      { id: 'llm-deepseek-account', config: { baseURL: platform.origin } },
    ]));
    await writeFile(join(home, '.credentials.yaml'), stringify({
      version: 1, refs: {}, records: {
        'deepseek-account-platform/default': {
          kind: 'grant', payload: { version: 1, token, issuer: platform.origin },
        },
      },
    }), { mode: 0o600 });
  } catch (error) {
    await Promise.all([outside.close(), platform?.close()]);
    throw error;
  }
  return {
    records,
    setMode(value) { mode = value; },
    setBalance(value) { balance = value; },
    close: () => Promise.all([platform.close(), outside.close()]),
  };
}

async function waitFor(description, condition, timeout = 15_000) {
  const deadline = Date.now() + timeout;
  do {
    const result = await condition();
    if (result) return result;
    await delay(100);
  } while (Date.now() < deadline);
  throw new Error('Timed out waiting for ' + description);
}

async function startDesktop(binary, host, home, documents) {
  const reservation = await serve((_, response) => response.writeHead(404).end());
  const debuggerURL = reservation.origin;
  await reservation.close();
  const env = {
    ...process.env, DSH_HOME: home, DSH_ACP_PATH: host, KWORK_DOCUMENTS_DIRECTORY: documents,
    DSH_TELEMETRY_DISABLED: '1',
    WEBVIEW2_USER_DATA_FOLDER: join(home, 'webview2'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=' + new URL(debuggerURL).port,
  };
  delete env.DEEPSEEK_API_KEY;
  delete env.OPENAI_API_KEY;
  const processHandle = spawn(binary, [], { cwd: documents, env, stdio: ['ignore', 'inherit', 'inherit'] });
  let launchError;
  const exited = once(processHandle, 'exit').catch(error => {
    launchError = error;
    return [null, null];
  });
  const running = () => processHandle.exitCode === null && processHandle.signalCode === null && !launchError;
  let browser;
  const terminate = async () => {
    if (running()) {
      processHandle.kill();
      await exited;
    }
    await browser?.close().catch(() => {});
  };
  try {
    await waitFor('WebView2 debugger', async () => {
      if (launchError) throw launchError;
      if (!running()) throw new Error('KWork exited before opening its window');
      return (await fetch(debuggerURL + '/json/version').catch(() => null))?.ok;
    }, 45_000);
    browser = await chromium.connectOverCDP(debuggerURL);
    const page = await waitFor('KWork main document', async () => {
      for (const candidate of browser.contexts().flatMap(context => context.pages())) {
        if (await candidate.evaluate(() => !!window.__TAURI__?.core).catch(() => false)) return candidate;
      }
    }, 45_000);
    page.setDefaultTimeout(30_000);
    return {
      page,
      async close() {
        const pid = processHandle.pid;
        assert(Number.isSafeInteger(pid));
        await promisify(execFile)('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
          `if (-not (Get-Process -Id ${pid}).CloseMainWindow()) { exit 1 }`]);
        await waitFor('KWork window to close', () => !running());
        const [code] = await exited;
        assert.equal(code, 0, 'KWork exited cleanly');
        await browser.close().catch(() => {});
      },
      terminate,
    };
  } catch (error) {
    await terminate();
    throw error;
  }
}

const button = (region, name) => region.getByRole('button', { name, exact: true });

async function accountSettings(page, firstBoot) {
  const onboarding = page.locator('#desktop-onboarding');
  if (firstBoot) {
    await button(onboarding, '开始设置').click();
    await button(onboarding, '跳过').click();
    await button(onboarding, '我知道了').click();
    await onboarding.waitFor({ state: 'hidden' });
  }
  await page.locator('#deepseek-account-launcher > button').click();
  await page.getByRole('menuitem', { name: '设置', exact: true }).click();
  await button(page.locator('#composer-submit-accessibility'), '账号与余额').dispatchEvent('click');
  const form = page.locator('#deepseek-account-form');
  await form.getByText('Platform Fixture', { exact: true }).waitFor();
  return form;
}

async function verify(binary, host, home, documents, fixture) {
  let desktop = await startDesktop(binary, host, home, documents);
  const count = (path) => fixture.records.filter(entry => entry.path === path).length;
  try {
    let account = await accountSettings(desktop.page, true);
    await verifyAgentPresets(desktop.page, documents);
    let platform = desktop.page.locator('#account-platform-view');
    const retry = button(platform, '重试');
    await account.getByRole('link', { name: '查询用量', exact: true }).click();
    await waitFor('blocked document request', () => count('/usage:blocked'));
    await retry.waitFor({ timeout: 8_000 });
    assert.equal(count('outside:/wrong-document'), 0, 'The rejected document never reached the other port');
    fixture.setMode('disconnect');
    await retry.click();
    await waitFor('disconnected document request', () => count('/usage:disconnect'));
    await retry.waitFor({ timeout: 8_000 });
    fixture.setMode('allow');
    await retry.click();
    await waitFor('usable usage page', () => count('/verified') === 1);
    await platform.locator('[data-part=status]').waitFor({ state: 'hidden' });
    await button(platform, '返回 KWork').click();
    await platform.waitFor({ state: 'hidden' });
    await account.getByRole('link', { name: '充值', exact: true }).click();
    await waitFor('usable top-up page', () => count('/verified') === 2);
    await platform.locator('[data-part=status]').waitFor({ state: 'hidden' });
    fixture.setBalance('12.50');
    await button(platform, '返回 KWork').click();
    await account.getByText('¥12.50', { exact: true }).waitFor();
    await desktop.close();

    desktop = await startDesktop(binary, host, home, documents);
    account = await accountSettings(desktop.page, false);
    platform = desktop.page.locator('#account-platform-view');
    await account.getByRole('link', { name: '查询用量', exact: true }).click();
    await waitFor('usage page after restart', () => count('/verified') === 3);
    await platform.locator('[data-part=status]').waitFor({ state: 'hidden' });
    await desktop.close();
  } finally {
    await desktop.terminate();
  }

  const documentsLoaded = fixture.records.filter(entry => entry.path.startsWith('/page/'));
  assert.deepEqual(documentsLoaded.map(entry => entry.path), ['/page/usage', '/page/top_up', '/page/usage']);
  assert(documentsLoaded.every(entry => !entry.ordinary), 'Each open clears cookies from the last visit');
  const verified = fixture.records.filter(entry => entry.path === '/verified');
  assert.deepEqual(verified.map(entry => entry.pref), ['empty', 'saved', 'saved']);
  assert(verified.every(entry => entry.auth && entry.ordinary), 'The current page has its own ordinary session cookie');
  for (const path of ['/static-asset', '/redirected-asset', '/fixture-worker.js', '/worker-fetch']) {
    const assets = fixture.records.filter(entry => entry.path === path);
    assert.equal(assets.length, 3, path);
    assert(assets.every(entry => entry.route && entry.deployment && entry.client === 'desktop-win'), path);
  }
  assert(documentsLoaded.every(entry => entry.route && entry.deployment && entry.client === 'desktop-win'));
  const external = fixture.records.filter(entry => entry.path.startsWith('outside:'));
  assert.equal(external.length, 3, 'Only redirected assets reached the other port');
  assert(external.every(entry => !entry.ordinary && !entry.route && !entry.deployment && !entry.auth && !entry.client),
    'The official request policy strips Cookie and other injected headers on cross-origin redirects');
  console.log('Windows embedded Electron smoke passed: recovery, usage, top-up, worker requests, redirect isolation and profile storage after restart.');
}

async function verifyAgentPresets(page, cwd) {
  const shell = page.locator('#settings-shell');
  await button(shell.getByRole('navigation'), 'Agent 预设').click();
  const panel = shell.locator('#settings-agent-presets');
  await panel.getByRole('heading', { name: 'Agent 预设', exact: true }).waitFor();
  await panel.getByRole('button', { name: '查看配置: 标准模式', exact: true }).click();
  const config = page.getByRole('dialog', { name: '查看配置 · 标准模式', exact: true });
  await config.locator('pre').waitFor();
  await page.keyboard.press('Escape');
  await config.waitFor({ state: 'hidden' });

  const result = await page.evaluate(async (cwd) => {
    const invoke = window.__TAURI__.core.invoke;
    const preset = request => invoke('agent_preset_settings', { request });
    const roster = await preset({ operation: 'list' });
    const created = await invoke('new_session', { cwd, agentPreset: 'minimal' });
    const switched = await preset({ operation: 'select', id: 'standard', sessionId: created.sessionId });
    const live = await preset({ operation: 'session', sessionId: created.sessionId });
    return { ids: roster.presets.filter(option => !option.broken).map(option => option.id),
      created: created.agentPreset, switched: switched.agentPreset, live };
  }, cwd);
  for (const id of ['standard', 'ptc', 'minimal', 'cordis']) assert(result.ids.includes(id), id);
  assert.equal(result.created, 'minimal');
  assert.equal(result.switched, 'standard');
  assert.deepEqual(result.live, { agentPreset: 'standard', canSelect: true });
  await button(shell.getByRole('navigation'), '账号与余额').click();
  await page.locator('#deepseek-account-form').getByText('Platform Fixture', { exact: true }).waitFor();
  console.log('Windows Agent presets passed: official list, YAML, explicit creation, in-session selection.');
}

if (import.meta.main) {
  assert.equal(process.platform, 'win32', 'Run the smoke test with the actual Windows WebView2 runtime');
  const binary = resolve(process.argv[2] ?? 'target/release/KWork.exe');
  await access(binary);
  const root = await mkdtemp(join(tmpdir(), 'kwork-platform-windows-'));
  const home = join(root, 'harness');
  const documents = join(root, 'documents');
  let fixture;
  let testFailed = false;
  try {
    await mkdir(documents);
    const host = process.argv[3] ? resolve(process.argv[3]) : await installHost(root);
    fixture = await platformFixture(home);
    await verify(binary, host, home, documents, fixture);
  } catch (error) {
    testFailed = true;
    if (fixture) console.error('Platform request checks:', JSON.stringify(fixture.records));
    throw error;
  } finally {
    try {
      await fixture?.close();
      await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 250 });
    } catch (error) {
      if (!testFailed) throw error;
      console.error('Could not remove the Windows fixture:', error);
    }
  }
}
