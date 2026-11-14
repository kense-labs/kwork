import assert from 'node:assert/strict';
import { test } from 'node:test';
import { AgentTerminalAttachment } from '../services/agent-terminal-bridge.mjs';

function channel() {
  const commands = [];
  let wake;
  return {
    input: { async *[Symbol.asyncIterator]() {
      while (true) {
        while (commands.length) yield commands.shift();
        await new Promise(resolve => { wake = resolve; });
      }
    } },
    push(command) { commands.push(command); wake?.(); },
  };
}

function fixture(frames, { end = false } = {}) {
  const uplink = channel();
  const abort = new AbortController();
  async function * sequence() {
    yield * frames;
    if (!end) await new Promise(resolve => {
      if (abort.signal.aborted) resolve();
      else abort.signal.addEventListener('abort', resolve, { once: true });
    });
  }
  const iterator = frames[Symbol.asyncIterator]?.() ?? sequence();
  const stream = {
    next: () => iterator.next(),
    return: async () => {
      abort.abort();
      await iterator.return?.();
      return { done: true };
    },
  };
  const controller = { ctx: { extend({ invocation }) {
    return { terminalController: { attach(_agent, id, signal) {
      assert.equal(id, 'terminal');
      assert.equal(signal.aborted, false);
      uplink.source = invocation.uplink();
      return stream;
    } } };
  } }, environment: () => ({ maxInputBytes: 8 }) };
  const agent = { id: 'session', ctx: controller.ctx };
  const events = [];
  const attachment = new AgentTerminalAttachment(controller, agent, 'terminal', event => events.push(event));
  return { attachment, events, uplink };
}

function fixtureFrames(frames, options) {
  const info = { id: 'terminal', title: 'Agent', cols: 80, rows: 24, state: 'running' };
  return fixture(frames.map(frame => ({ info, ...frame, ...(frame.info ? { info: { ...info, ...frame.info } } : {}) })), options);
}

test('publishes a snapshot before accepting ordered input and acknowledgements', async () => {
  const { attachment, events, uplink } = fixtureFrames([
    { type: 'snapshot', sequence: 2, screen: 'retained', writable: true },
  ]);
  const attached = await attachment.start();
  assert.equal(attached.frame.screen, 'retained');
  assert.equal(events[0].kind, 'agent_terminal_frame');
  const pump = attachment.pump();
  const command = attachment.command({ type: 'write', data: 'ok', typed: true, sequence: 1 });
  assert.equal((await uplink.source.next()).value.typed, true);
  await assert.rejects(attachment.command({ type: 'resize', cols: 80, rows: 24, sequence: 3 }), /序号/);
  await attachment.close();
  await pump;
  await assert.rejects(command, /关闭/);
  assert.equal(attachment.commands.length, 0);
});

test('bounds unacknowledged input bytes and releases capacity on ack', async () => {
  const { attachment, uplink } = fixtureFrames([
    { type: 'snapshot', sequence: 0, screen: '', writable: true },
    { type: 'ack', sequence: 1 },
  ]);
  await attachment.start();
  const pump = attachment.pump();
  const first = attachment.command({ type: 'write', data: '12345678', typed: false, sequence: 1 });
  await uplink.source.next();
  await assert.rejects(attachment.command({ type: 'write', data: 'x', typed: true, sequence: 2 }), /尚未确认/);
  assert.deepEqual(await first, { sequence: 1, refusal: undefined });
  await attachment.close();
  await pump;
});

test('forwards output and state without exposing protocol ack frames', async () => {
  const delivered = Promise.withResolvers();
  const { attachment, events } = fixtureFrames([
    { type: 'snapshot', sequence: 0, screen: '', writable: true },
    { type: 'output', sequence: 1, data: 'hello' },
    { type: 'state', info: { control: 'user' }, writable: true },
  ]);
  const original = attachment.emit;
  attachment.emit = event => {
    original(event);
    if (event.frame.type === 'state') delivered.resolve();
  };
  await attachment.start();
  const pump = attachment.pump();
  await delivered.promise;
  await attachment.close();
  await pump;
  assert.deepEqual(events.map(event => [event.kind, event.frame.type]), [
    ['agent_terminal_frame', 'snapshot'], ['agent_terminal_frame', 'output'], ['agent_terminal_state', 'state'],
  ]);
});

test('a naturally ended attachment remains visible and becomes explicitly reconnectable', async () => {
  const { attachment, events } = fixtureFrames([
    { type: 'snapshot', sequence: 0, screen: 'retained', writable: true },
  ], { end: true });
  await attachment.start();
  await attachment.pump();
  assert.equal(events.at(-1).kind, 'agent_terminal_error');
  assert.match(events.at(-1).error, /重新连接/);
  await assert.rejects(attachment.command({ type: 'write', data: 'x', typed: true, sequence: 1 }), /关闭/);
});

test('close wakes a blocked input iterator and rejects pending commands', async () => {
  const { attachment, uplink } = fixtureFrames([
    { type: 'snapshot', sequence: 0, screen: '', writable: true },
  ]);
  await attachment.start();
  const pump = attachment.pump();
  const pending = attachment.command({ type: 'resize', cols: 100, rows: 40, sequence: 1 });
  const consumed = await uplink.source.next();
  assert.equal(consumed.value.type, 'resize');
  await attachment.close();
  await pump;
  await assert.rejects(pending, /关闭/);
  await assert.rejects(attachment.command({ type: 'write', data: 'x', typed: true, sequence: 2 }), /关闭/);
});

test('a failed stream reports an actionable error and rejects pending input', async () => {
  async function * failed() {
    yield { type: 'snapshot', sequence: 0, screen: '', info: { cols: 80, rows: 24 }, writable: true };
    throw new Error('fixture disconnected');
  }
  const { attachment, events, uplink } = fixture(failed());
  await attachment.start();
  const pump = attachment.pump();
  const pending = attachment.command({ type: 'write', data: 'x', typed: true, sequence: 1 });
  await uplink.source.next();
  await pump;
  await assert.rejects(pending, /disconnected/);
  assert.equal(events.at(-1).kind, 'agent_terminal_error');
  assert.match(events.at(-1).error, /disconnected/);
});
