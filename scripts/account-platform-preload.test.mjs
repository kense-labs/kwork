import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../src-tauri/src/account_platform_preload.js', import.meta.url), 'utf8');
const origin = 'https://platform.example.invalid';

function install(headers, { pageOrigin = origin, nested = false } = {}) {
  const requests = [];
  const fetch = async (input, options) => {
    const request = new Request(input, options);
    requests.push(request);
    return new Response(null);
  };
  const window = { fetch };
  window.top = nested ? {} : window;
  const data = { origin, token: 'synthetic-grant', headers, version: '1.2.3' };
  runInNewContext(`(${source})(data)`, {
    data, window, Request, URL, location: { origin: pageOrigin }, navigator: { platform: 'MacIntel' },
  });
  return { window, fetch, requests };
}

test('native request interception leaves the JavaScript request unchanged', async () => {
  const { window, fetch, requests } = install(null);
  assert.equal(window.fetch, fetch);
  assert.equal(window.dsh.getAuthToken(), 'synthetic-grant');
  assert.equal(window.dsh.getLocale(), 'zh_CN');
  await window.fetch(`${origin}/usage`);
  assert.equal(requests[0].headers.has('x-client-platform'), false);
  assert.equal(requests[0].redirect, 'follow');
});

test('fetch fallback injects headers on its exact origin', async () => {
  const { window, requests } = install({ 'x-deployment': 'synthetic-route' });
  await window.fetch(`${origin}/usage`);
  assert.equal(requests[0].headers.get('x-deployment'), 'synthetic-route');
  assert.equal(requests[0].headers.get('x-client-platform'), 'desktop-mac');
  assert.equal(requests[0].headers.get('x-client-version'), '1.2.3');
  assert.equal(requests[0].mode, 'same-origin');
  assert.equal(requests[0].redirect, 'follow');
  await window.fetch('https://platform.example.invalid:8443/usage');
  assert.equal(requests[1].headers.has('x-deployment'), false);
  assert.equal(requests[1].mode, 'cors');
  assert.equal(requests[1].redirect, 'follow');
});

test('the scoped request preserves caller redirect controls and its body', async () => {
  const { window, requests } = install({ 'x-deployment': 'synthetic-route' });
  for (const redirect of ['manual', 'error']) {
    await window.fetch(new Request(`${origin}/usage`, {
      method: 'POST', headers: { 'content-type': 'text/plain' }, body: 'fixture',
      mode: 'no-cors', redirect,
    }));
  }
  for (const [index, request] of requests.entries()) {
    assert.equal(request.mode, 'same-origin');
    assert.equal(request.redirect, index === 0 ? 'manual' : 'error');
    assert.equal(request.method, 'POST');
    assert.equal(request.headers.get('x-deployment'), 'synthetic-route');
    assert.equal(await request.text(), 'fixture');
  }
});

test('platform bootstrap is only visible to the trusted top frame', () => {
  for (const options of [{ nested: true }, { pageOrigin: 'https://platform.example.invalid:8443' }]) {
    const { window, fetch } = install(null, options);
    assert.equal(window.dsh, undefined);
    assert.equal(window.fetch, fetch);
  }
});
