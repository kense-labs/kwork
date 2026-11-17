import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';

// Run the real bridge in an isolated Harness home, then restart against that same registry.
const service = resolve(process.argv[2] ?? '../dsh-acp');
const fixtures = await mkdtemp(join(tmpdir(), 'kwork-workspace-registry-'));
const preferences = join(fixtures, 'preferences.json');
const home = join(fixtures, 'harness');
const entry = join(fixtures, 'bridge.mjs');
const vendor = fileURLToPath(new URL('../services/vendor/dsh-schedule/index.mjs', import.meta.url));
const method = '_dsh/gpui/workspace-registry';

async function runHost(workspace, verify, { homePath = home, preferencesPath = preferences, documentsPath } = {}) {
  const env = { ...process.env, DSH_HOME: homePath, KWORK_WORKSPACE: workspace,
    KWORK_SCHEDULE_MODULE: vendor, KWORK_WORKSPACE_PREFERENCES: preferencesPath };
  delete env.KWORK_TERMINAL_MODULE;
  delete env.KWORK_TERMINAL_RUNTIME;
  delete env.KWORK_DOCUMENTS_DIRECTORY;
  if (documentsPath) env.KWORK_DOCUMENTS_DIRECTORY = documentsPath;
  const host = spawn(process.execPath, [entry, service, join(service, 'apps/cli/lib/profile-boot.js')], {
    cwd: service, env, stdio: ['pipe', 'pipe', 'inherit'],
  });
  const exited = once(host, 'exit');
  const notifications = [];
  const replies = new Map();
  const outgoing = [];
  let nextId = 1;
  const stdout = createInterface({ input: host.stdout });
  stdout.on('line', line => {
    const frame = JSON.parse(line);
    if (frame.method === `${method}-changed`) notifications.push(frame.params);
    const waiting = replies.get(frame.id);
    if (!waiting) return;
    clearTimeout(waiting.timer);
    replies.delete(frame.id);
    if (frame.error) waiting.reject(new Error(JSON.stringify(frame.error)));
    else waiting.resolve(frame.result);
  });
  function request(name, params) {
    const id = nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        replies.delete(id);
        reject(new Error(`Timed out in ${basename(homePath)}: ${name}${params?.operation?.action ? ` (${params.operation.action})` : ''}`));
      }, 15000);
      replies.set(id, { resolve, reject, timer });
      outgoing.push(JSON.stringify({ jsonrpc: '2.0', id, method: name, params }) + '\n');
      if (outgoing.length === 1) queueMicrotask(() => host.stdin.write(outgoing.splice(0).join('')));
    });
  }
  try {
    await request('initialize', { protocolVersion: 1, clientCapabilities: {} });
    await verify(operation => request(method, { operation }), notifications, request);
  } finally {
    host.stdin.end();
    const timeout = setTimeout(() => host.kill('SIGKILL'), 3000);
    const [code, signal] = await exited;
    clearTimeout(timeout);
    assert.equal(signal, null);
    assert.equal(code, 0);
  }
}

try {
  const status = await readFile(new URL('../services/session-read-status.mjs', import.meta.url), 'utf8');
  const bridge = await readFile(new URL('../services/acp-session-bridge.mjs', import.meta.url), 'utf8');
  const terminal = fileURLToPath(new URL('../services/agent-terminal-bridge.mjs', import.meta.url));
  await writeFile(entry, `${status}\nconst bundledAskUser = {};\n${bridge.replace("'./agent-terminal-bridge.mjs'", JSON.stringify(terminal))}`);
  const dirs = {};
  for (const name of ['parent', 'parent/child', 'sibling', 'hidden', 'late']) {
    const path = join(fixtures, name);
    await mkdir(path, { recursive: true });
    dirs[name] = await realpath(path);
  }
  const parent = dirs.parent;
  const child = dirs['parent/child'];
  const sibling = dirs.sibling;
  const retainedFile = join(parent, 'keep-me.txt');
  await writeFile(retainedFile, 'leave workspace files in place');
  await writeFile(preferences, JSON.stringify({
    addedWorkspaces: [parent, child, dirs.hidden, join(fixtures, 'missing')],
    workspaceOrder: [child, parent], hidden: [dirs.hidden], aliases: { [parent]: 'Migrated parent' },
  }));
  let committed;
  let groupedId;
  let ungroupedId;
  let forkId;
  let removedId;
  const paths = snapshot => snapshot.items.map(item => item.path);
  const members = (snapshot, path) => snapshot.items.find(item => item.path === path).sessionIds;
  const archive = (request, action, sessionId) => request('_dsh/gpui/session/archive', { action, sessionId });
  const waitForSnapshot = async (notifications, matches) => {
    for (let attempts = 0; !notifications.some(matches); attempts++) {
      assert.ok(attempts < 60, 'the committed registry state should be broadcast');
      await new Promise(resolve => setTimeout(resolve, 25));
    }
  };
  await runHost(parent, async (invoke, notifications, request) => {
    const migrated = await invoke({ action: 'snapshot' });
    assert.equal(migrated.available, true);
    assert.deepEqual(paths(migrated), [child, parent]);
    assert.equal(migrated.items[1].title, 'Migrated parent');
    assert.deepEqual((await invoke({ action: 'create', path: parent })).items, migrated.items);
    const created = await invoke({ action: 'create', path: sibling });
    assert.deepEqual(paths(created), [sibling, child, parent]);
    const [source, anchor] = created.items;
    const moved = await invoke({ action: 'move', id: source.id, beforeId: anchor.id });
    assert.deepEqual(moved.items, created.items);
    const reordered = await invoke({ action: 'move', id: source.id });
    assert.deepEqual(paths(reordered), [child, parent, sibling]);
    await assert.rejects(invoke({ action: 'move', id: source.id, beforeId: 'missing' }), /missing/);
    assert.deepEqual((await invoke({ action: 'snapshot' })).items, reordered.items);
    await waitForSnapshot(notifications, snapshot => JSON.stringify(snapshot.items) === JSON.stringify(reordered.items));

    const target = reordered.items.find(item => item.path === parent);
    const createSession = () => request('session/new', { cwd: parent, mcpServers: [] });
    groupedId = (await createSession()).sessionId;
    ungroupedId = (await createSession()).sessionId;
    const attach = id => invoke({ action: 'attach-session', id, sessionId: groupedId });
    await assert.rejects(attach(source.id), /cwd resolves/);
    assert.deepEqual(members(await attach(target.id), parent), [groupedId]);
    assert.deepEqual(members(await attach(target.id), parent), [groupedId]);
    const fork = await request('_dsh/gpui/session/fork', { sessionId: groupedId, atSeq: 0, allowMigration: true });
    forkId = fork.sessionId;
    assert.ok(forkId);
    assert.deepEqual(members(fork.workspaceRegistry, parent), [forkId, groupedId]);
    const looseFork = await request('_dsh/gpui/session/fork', { sessionId: ungroupedId, atSeq: 0, allowMigration: true });
    assert.ok(looseFork.sessionId);
    assert.deepEqual(members(await invoke({ action: 'snapshot' }), parent), [forkId, groupedId]);

    const pin = sessionId => invoke({ action: 'pin', sessionId });
    assert.deepEqual((await pin(ungroupedId)).pinned, [ungroupedId]);
    assert.deepEqual((await pin(forkId)).pinned, [forkId, ungroupedId]);
    assert.deepEqual((await pin(ungroupedId)).pinned, [forkId, ungroupedId]);
    await assert.rejects(pin('missing-session'), /missing-session/);
    await waitForSnapshot(notifications, snapshot => snapshot.pinned[0] === forkId);
    const archived = await archive(request, 'archive', forkId);
    assert.deepEqual(archived.pinned, [ungroupedId]);
    assert.deepEqual(archived.archived, [forkId]);
    assert.deepEqual(members(archived, parent), [forkId, groupedId]);
    await assert.rejects(pin(forkId), /archiv/i);
    await waitForSnapshot(notifications, snapshot => snapshot.archived.includes(forkId)
      && !snapshot.pinned.includes(forkId) && members(snapshot, parent).includes(forkId));

    const rename = (id, title) => invoke({ action: 'rename', id, title });
    await assert.rejects(rename(target.id, '   '), /workspace\/invalid-name/);
    await assert.rejects(rename(target.id, anchor.title), /workspace\/name-conflict/);
    const simultaneous = await Promise.allSettled([
      rename(target.id, '  Renamed workspace  '),
      rename(source.id, 'Renamed workspace'),
    ]);
    assert.deepEqual(simultaneous.map(result => result.status), ['fulfilled', 'rejected']);
    assert.match(simultaneous[1].reason.message, /workspace\/name-conflict/);
    assert.equal(simultaneous[0].value.items.find(item => item.id === target.id).title, 'Renamed workspace');
    await waitForSnapshot(notifications, snapshot => snapshot.items.some(item =>
      item.id === target.id && item.title === 'Renamed workspace'));
    committed = await invoke({ action: 'snapshot' });
  });
  const checkpoint = JSON.parse(await readFile(join(home, 'storages/session_projcache/sessions', `${groupedId}.json`), 'utf8'));
  const activity = checkpoint.record.rows.sessionListMetadata;
  assert.equal(activity?.ver, 1);
  assert.deepEqual(activity.val, { blank: true, lastPromptAt: null });
  await writeFile(preferences, JSON.stringify({ addedWorkspaces: [dirs.late, sibling],
    workspaceOrder: [sibling, parent, child], pinned: [forkId, groupedId] }));
  await runHost(parent, async invoke => {
    const restarted = await invoke({ action: 'snapshot' });
    assert.deepEqual(restarted.items, committed.items);
    assert.deepEqual(restarted.pinned, [ungroupedId]);
    assert.deepEqual(restarted.archived, [forkId]);
  });
  await rm(join(home, 'kwork-workspace-groups-v1.json'));
  await rm(join(home, 'kwork-session-pins-v1.json'));
  await writeFile(preferences, JSON.stringify({
    addedWorkspaces: [parent, sibling], workspaceOrder: [sibling, parent], aliases: { [parent]: 'Local alias' },
    pinned: [groupedId, forkId, 'missing-session'],
  }));
  await runHost(parent, async (invoke, _, request) => {
    const migrated = await invoke({ action: 'snapshot' });
    assert.deepEqual(migrated.items, committed.items);
    assert.deepEqual(migrated.pinned, [groupedId, ungroupedId]);
    const restored = await archive(request, 'restore', forkId);
    assert.deepEqual(restored.archived, []);
    assert.deepEqual(restored.pinned, [groupedId, ungroupedId]);
    assert.deepEqual(members(restored, parent), [forkId, groupedId]);
    assert.deepEqual((await invoke({ action: 'unpin', sessionId: groupedId })).pinned, [ungroupedId]);
  });
  await runHost(parent, async (invoke, notifications, request) => {
    const restarted = await invoke({ action: 'snapshot' });
    assert.deepEqual(restarted.items, committed.items);
    assert.deepEqual(restarted.pinned, [ungroupedId]);
    assert.deepEqual(restarted.archived, []);
    removedId = restarted.items.find(item => item.path === parent).id;
    const removed = await invoke({ action: 'delete', id: removedId });
    assert.deepEqual(paths(removed), [child, sibling]);
    assert.deepEqual(removed.pinned, [ungroupedId]);
    await assert.rejects(invoke({ action: 'delete', id: removedId }), /workspace\/not-found/);
    await waitForSnapshot(notifications, snapshot => !snapshot.items.some(item => item.id === removedId));
    assert.equal(await readFile(retainedFile, 'utf8'), 'leave workspace files in place');
    const { sessions } = await request('session/list', { cwd: parent });
    assert.ok([groupedId, forkId].every(id => sessions.some(session => session.sessionId === id)));
  });
  await runHost(parent, async (invoke, _, request) => {
    const restarted = await invoke({ action: 'snapshot' });
    assert.deepEqual(paths(restarted), [child, sibling]);
    const recreated = await invoke({ action: 'create', path: parent });
    assert.notEqual(recreated.items[0].id, removedId);
    assert.deepEqual(members(recreated, parent), []);
    await assert.rejects(invoke({ action: 'rename', id: removedId, title: 'Stale title' }), /workspace\/not-found/);
    await assert.rejects(invoke({ action: 'delete', id: removedId }), /workspace\/not-found/);
    const { sessions } = await request('session/list', { cwd: parent });
    assert.ok([groupedId, forkId].every(id => sessions.some(session => session.sessionId === id)));
  });

  const emptyPrefs = join(fixtures, 'first-start-preferences.json');
  const clean = { homePath: join(fixtures, 'first-start-home'), preferencesPath: emptyPrefs };
  const documentsPath = join(fixtures, 'system-documents');
  const defaultFolder = join(documentsPath, 'deepseek-harness', 'default-workspace');
  const initialize = invoke => invoke({ action: 'initialize-default' });
  let defaultItem;
  await runHost(parent, async invoke => {
    await assert.rejects(initialize(invoke), /workspace\/documents-unavailable/);
    assert.deepEqual((await invoke({ action: 'snapshot' })).items, []);
  }, clean);
  await runHost(parent, async (invoke, notifications, request) => {
    assert.deepEqual((await invoke({ action: 'snapshot' })).items, []);
    const [first, second] = await Promise.all([initialize(invoke), initialize(invoke)]);
    assert.equal(first.path, await realpath(defaultFolder));
    assert.deepEqual(first.items, second.items);
    assert.equal(first.items.length, 1);
    [defaultItem] = first.items;
    assert.equal(first.workspaceId, defaultItem.id);
    assert.equal(defaultItem.title, 'default-workspace');
    assert.deepEqual(defaultItem.sessionIds, []);
    assert.deepEqual((await request('session/list', {})).sessions, []);
    await waitForSnapshot(notifications, snapshot => snapshot.items.some(item => item.id === defaultItem.id));
  }, { ...clean, documentsPath });
  await runHost(parent, async invoke => {
    assert.deepEqual((await initialize(invoke)).items, [defaultItem]);
    await invoke({ action: 'delete', id: defaultItem.id });
    assert.equal((await initialize(invoke)).path, null);
  }, clean);
  await runHost(parent, async invoke => {
    const reply = await initialize(invoke);
    assert.equal(reply.path, null);
    assert.deepEqual(reply.items, []);
    assert.equal(await realpath(defaultFolder), defaultItem.path);
  }, { ...clean, documentsPath });

  const withHistory = { homePath: join(fixtures, 'history-only-home'), preferencesPath: emptyPrefs };
  let historyId;
  await runHost(parent, async (invoke, _, request) => {
    historyId = (await request('session/new', { cwd: parent, mcpServers: [] })).sessionId;
    assert.equal((await initialize(invoke)).path, null);
  }, withHistory);
  await runHost(parent, async (invoke, _, request) => {
    assert.ok((await request('session/list', {})).sessions.some(session => session.sessionId === historyId));
    const reply = await initialize(invoke);
    assert.equal(reply.path, null);
    assert.equal(reply.workspaceId, null);
    assert.deepEqual(paths(reply), [parent]);
    assert.deepEqual(members(reply, parent), [historyId]);
  }, withHistory);
  console.log('Workspace registry smoke passed: order, membership, pin, rename, removal, migration, first use and restart');
} finally {
  await rm(fixtures, { recursive: true, force: true });
}
