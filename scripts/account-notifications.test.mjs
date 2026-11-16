import assert from 'node:assert/strict';
import { test } from 'bun:test';
import { createNotificationController } from '../ui/account-notifications.js';

const notice = (id, title = id, displayUntil = null) => ({ id, title, body: '', actions: [], displayUntil });
const batch = (notifications, nextPollAfterSeconds = 2) => ({ notifications, nextPollAfterSeconds });
const flush = async () => { for (let i = 0; i < 4; i += 1) await Promise.resolve(); };

function fixture() {
  let at = Date.UTC(2026, 0, 1);
  let seq = 0;
  const timers = new Map();
  const reads = [];
  const contents = [];
  const events = [];
  let cards = [];
  function deferred(target, data) {
    return new Promise((resolve, reject) => target.push({ ...data, resolve, reject }));
  }
  const controller = createNotificationController({
    read: (scope, locale) => deferred(reads, { scope, locale }),
    readContent: (scope, id, locale) => deferred(contents, { scope, id, locale }),
    report: async (scope, id, event, locale) => { events.push([scope, id, event, locale]); return true; },
    publish: notices => { cards = notices; },
    now: () => at,
    clock: {
      setTimeout(callback, delay) { const id = ++seq; timers.set(id, { callback, at: at + delay }); return id; },
      clearTimeout(id) { timers.delete(id); },
    },
  });
  async function tick(elapsed) {
    const end = at + elapsed;
    for (;;) {
      const next = [...timers].filter(([, task]) => task.at <= end).sort(([, a], [, b]) => a.at - b.at)[0];
      if (!next) break;
      at = next[1].at;
      timers.delete(next[0]);
      next[1].callback();
      await flush();
    }
    at = end;
  }
  return { controller, reads, contents, events, tick, at: () => at, cards: () => cards,
    begin: (scope = 'first', mode = 'modern') => controller.setScope({ scope, mode, pollIntervalMs: 600_000 }) };
}

test('retains the painted first card when a refresh omits it and sends each interaction separately', async () => {
  const f = fixture();
  f.begin();
  f.reads[0].resolve(batch([notice('a'), notice('b')]));
  await flush();
  f.controller.presented('a');
  f.controller.presented('a');
  f.controller.click('a');
  f.controller.refresh();
  f.reads[1].resolve(batch([notice('b'), notice('c')]));
  await flush();
  assert.deepEqual(f.cards().map(card => card.id), ['a', 'b', 'c']);
  assert.equal(f.controller.dismiss('b'), false);
  assert.equal(f.controller.dismiss('a'), true);
  f.controller.presented('b');
  assert.deepEqual(f.events.map(([, id, event]) => [id, event]),
    [['a', 'displayed'], ['a', 'cta_clicked'], ['a', 'dismissed'], ['b', 'displayed']]);
});

test('expiry promotes without dismissal and server delay is measured after the query settles', async () => {
  const f = fixture();
  f.begin();
  await f.tick(20_000);
  assert.equal(f.reads.length, 1);
  const expires = new Date(f.at() + 500).toISOString();
  f.reads[0].resolve(batch([notice('a', 'short', expires), notice('b')]));
  await flush();
  await f.tick(500);
  assert.equal(f.cards()[0].id, 'b');
  assert.deepEqual(f.events, []);
  await f.tick(1499);
  assert.equal(f.reads.length, 1);
  await f.tick(1);
  assert.equal(f.reads.length, 2);
  f.reads[1].resolve(batch([], 3));
  await flush();
  await f.tick(2900);
  assert.equal(f.reads.length, 2);
  await f.tick(100);
  assert.equal(f.reads.length, 3);
});

test('an old read or localized reply cannot repopulate a replaced account or a dismissed card occurrence', async () => {
  const f = fixture();
  f.begin();
  f.begin('second');
  f.reads[0].resolve(batch([notice('stale')]));
  f.reads[1].resolve(batch([notice('a')]));
  await flush();
  assert.deepEqual(f.cards().map(card => card.id), ['a']);
  f.controller.setLocale('en-US');
  f.controller.dismiss('a');
  f.controller.refresh();
  f.reads[2].resolve(batch([notice('a', 'reoffered')]));
  await flush();
  f.contents[0].resolve(notice('a', 'old text'));
  await flush();
  assert.equal(f.cards()[0].title, 'reoffered');
  f.controller.setLocale('ja-JP');
  f.controller.setLocale('zh-CN');
  f.contents[2].resolve(notice('a', 'new language'));
  f.contents[1].resolve(notice('a', 'superseded language'));
  await flush();
  assert.equal(f.cards()[0].title, 'new language');
  f.controller.setScope(undefined);
  await f.tick(600_001);
  assert.deepEqual(f.cards(), []);
  assert.equal(f.reads.length, 3);
});

test('a legacy bonus uses the displayed acknowledgement exclusively', async () => {
  const f = fixture();
  f.begin('legacy', 'bonus');
  f.reads[0].resolve(batch([notice('b')]));
  await flush();
  f.controller.presented('b');
  f.controller.click('b');
  f.controller.dismiss('b');
  assert.deepEqual(f.events, [['legacy', 'b', 'displayed', 'zh-CN']]);
});

test('a dismissal before the first painted frame sends only dismissal', async () => {
  const f = fixture();
  f.begin();
  f.reads[0].resolve(batch([notice('a')]));
  await flush();
  f.controller.dismiss('a');
  f.controller.presented('a');
  assert.deepEqual(f.events, [['first', 'a', 'dismissed', 'zh-CN']]);
});
