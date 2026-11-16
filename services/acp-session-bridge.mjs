//! Keep standard ACP on stdio and route GPUI's session-only operations to the
//! same Cordis services used by the official Harness.
import { once } from 'node:events';
import { randomUUID } from 'node:crypto';
import { createReadStream, existsSync, readFileSync } from 'node:fs';
import { lstat, mkdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, isAbsolute, join } from 'node:path';
import { createInterface } from 'node:readline';
import { PassThrough } from 'node:stream';
import { AgentTerminalAttachment, prepareTerminalProfile, terminalInfo } from './agent-terminal-bridge.mjs';
import { pathToFileURL } from 'node:url';
import { stripVTControlCharacters } from 'node:util';

let notificationScope = randomUUID();
let legacyNotificationAccount;
const notificationEvents = new Set(['displayed', 'cta_clicked', 'dismissed']);

const [service, profileEntry, ...patchFiles] = process.argv.slice(2);
const require = createRequire(join(service, 'apps/cli/package.json'));
const { loadLayeredEnv, StartupError, getDshRuntimeVersion } =
  await import(pathToFileURL(require.resolve('@deepseek-ai/dsh-app-boot')).href);
const { runProfile } = await import(pathToFileURL(profileEntry).href);
const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
const acpInput = new PassThrough();
// Register before an early ACP client can write. Mixing this Interface's line
// events with a later async iterator leaves its already-buffered first line
// undelivered, so the bridge uses one FIFO from the beginning.
const inputLines = [];
let inputWake;
let inputEnded = false;
input.on('line', line => {
  inputLines.push(line);
  inputWake?.();
});
input.once('close', () => {
  inputEnded = true;
  inputWake?.();
});

// The official ACP transport still consumes the normal process stdin stream.
// GPUI's reserved frames are removed before they reach the SDK's ACP parser.
Object.defineProperty(process, 'stdin', { value: acpInput, configurable: true });

const { resolveDshHome } = await import(pathToFileURL(
  require.resolve('@deepseek-ai/dsh-home-paths'),
).href);
if (process.env.KWORK_TERMINAL_MODULE) {
  try {
    await prepareTerminalProfile({
      service,
      module: process.env.KWORK_TERMINAL_MODULE,
      home: resolveDshHome(),
      runtime: process.env.KWORK_TERMINAL_RUNTIME,
    });
  } catch (error) {
    // The agent-terminal integration is optional: a missing or mismatched
    // runtime must not take down the ACP host and every conversation with it.
    console.error(`KWork: Agent 终端不可用：${error?.message ?? error}`);
    delete process.env.KWORK_TERMINAL_MODULE;
  }
}
const ready = runProfile({
  environment: loadLayeredEnv('dsh', service),
  profile: 'acp',
  patchFiles,
  args: [],
}).then(async runtime => {
  startSessionReadStatus(runtime.ctx);
  try {
    await registerSessionListMetadata(runtime.ctx);
  } catch (error) {
    console.error(`KWork: 无法登记会话活动投影：${error?.message ?? error}`);
  }
  observeSessionUpdates(runtime.ctx);
  observeModelProviders(runtime.ctx);
  observeDeepSeekAccount(runtime.ctx);
  observePlatformSession(runtime.ctx);
  runtime.ctx.on('settings/document-updated', namespace => {
    if (namespace === 'ui-conversation') {
      process.stdout.write('{"jsonrpc":"2.0","method":"_dsh/gpui/composer-preference-changed"}\n');
    }
  });
  observePluginInstall(runtime.ctx);
  // KWork registers its linked terminal packages before the first launch. ACP
  // computes its interception map before reading them, so publish that public
  // mapping again before resolving imports from the linked package roots.
  if (process.env.KWORK_TERMINAL_MODULE) await runtime.ctx.get('pluginPackages')?.refresh();
  try {
    await startTerminalController(runtime.ctx);
  } catch (error) {
    console.error(`KWork: Agent 终端不可用：${error?.message ?? error}`);
  }
  await startSchedule(runtime.ctx);
  try {
    await importWorkspaceGroups(runtime.ctx);
  } catch (error) {
    console.error(`KWork: 工作区导入未完成：${error?.message ?? error}`);
  }
  try {
    await importPinnedSessions(runtime.ctx);
  } catch (error) {
    console.error(`KWork: 置顶会话导入未完成：${error?.message ?? error}`);
  }
  observeWorkspaceRegistry(runtime.ctx);
  return runtime;
});

const recordMethod = '_dsh/gpui/session/feedback';
const flushMethod = '_dsh/gpui/session/flush';
const forkMethod = '_dsh/gpui/session/fork';
const renameMethod = '_dsh/gpui/session/rename';
const planModeMethod = '_dsh/gpui/session/plan-mode';
const planReviewMethod = '_dsh/gpui/session/plan-review';
const permissionModeMethod = '_dsh/gpui/session/permission-mode';
const sessionCommandsMethod = '_dsh/gpui/session/commands';
const executeSessionCommandMethod = '_dsh/gpui/session/command-execute';
const runtimeSettingsMethod = '_dsh/gpui/runtime-settings';
const webSettingsMethod = '_dsh/gpui/web-settings';
const pluginCenterMethod = '_dsh/gpui/plugin-center';
const pluginInstallMethod = '_dsh/gpui/plugin-install';
const scheduleMethod = '_dsh/gpui/schedule';
const openHostSessionMethod = '_dsh/gpui/session/open';
const searchSessionsMethod = '_dsh/gpui/session/search';
const modelCatalogMethod = '_dsh/gpui/model-catalog';
const deepSeekAccountMethod = '_dsh/gpui/deepseek-account';
const accountNotificationsMethod = '_dsh/gpui/account-notifications';
const platformSessionMethod = '_dsh/gpui/account-platform-session';
const sessionArchiveMethod = '_dsh/gpui/session/archive';
const workspaceRegistryMethod = '_dsh/gpui/workspace-registry';
const sessionReadStateMethod = '_dsh/gpui/session/read-state';
const modelProviderMethods = new Set([
  '_dsh/gpui/providers/list', '_dsh/gpui/providers/save',
  '_dsh/gpui/providers/remove', '_dsh/gpui/providers/discover', '_dsh/gpui/providers/authorization',
]);
const providerAuthAttempts = new Map();
const providerAuthLogouts = new Set();
let accountCallback;
let accountCallbackFiber;
let accountLoginService;
const questionStateMethod = '_dsh/gpui/session/questions';
const questionDecisionMethod = '_dsh/gpui/session/question-decision';
// The ACP prompt slot is single-flight, so a message composed while the agent
// is busy cannot ride `session/prompt`. It becomes a durable Inbox item instead
// (official ComposerSubmissionPolicy): `queue` appends a turn, `steer` joins
// the running turn at its next step boundary.
const queuePromptMethod = '_dsh/gpui/session/queue-prompt';
const composerPreferenceMethod = '_dsh/gpui/composer-preference';
const inboxStateMethod = '_dsh/gpui/session/inbox-state';
const inboxActionMethod = '_dsh/gpui/session/inbox-action';
// The composer's GoalBar reads the same live goal view the official client
// projects, and its pause/resume/clear verbs are the goal service's own.
const goalStateMethod = '_dsh/gpui/session/goal-state';
const goalActionMethod = '_dsh/gpui/session/goal-action';
const teamStateMethod = '_dsh/gpui/session/team-state';
const terminalWatchMethod = '_dsh/gpui/terminal-watch';
const terminalInputMethod = '_dsh/gpui/terminal-input';
const terminalAttachMethod = '_dsh/gpui/terminal-attach';
const terminalDetachMethod = '_dsh/gpui/terminal-detach';
const terminalCloseMethod = '_dsh/gpui/terminal-close';
const terminalRenameMethod = '_dsh/gpui/terminal-rename';
const terminalWatches = new Map();
const terminalAttachments = new Map();
let loadingTerminalController;
let terminalTools;
// Non-human messages either wake a Turn (official TurnTriggerNodeView) or are
// injected into model history (official ContextInjectionRow). Only the waking
// producers get a trigger card; the rest are collapsed context rows, exactly
// like the official projection distinguishes them.
// Compaction summaries are logged just before the checkpoint message that
// replaces the shadowed history; cache them so the live marker can cite the
// official 已压缩 N 条历史记录 count like a reopened session does.
const compactionSummaries = new Map()
// Durable agent inbox per session: a non-human message is a turn trigger only
// when the inbox claimed it for the next turn (official `waking`); a notice that
// arrives mid-turn stays a collapsed context row, exactly like the replay does.
const inboxes = new WeakMap()
// Tool call whose arguments are still streaming in the current assistant
// message (official `summary.preparing`): only the name changes are forwarded.
const preparingTools = new WeakMap()
const planReviews = new Map();
const questionRequests = new Map();
const messageMethods = new Map([
  ['_dsh/gpui/message-feedback/list', 'list'],
  ['_dsh/gpui/message-feedback/put', 'put'],
  ['_dsh/gpui/message-feedback/delete', 'delete'],
]);
const extensionMethods = [
  recordMethod, flushMethod, forkMethod, renameMethod, planModeMethod, planReviewMethod,
  permissionModeMethod, sessionCommandsMethod, executeSessionCommandMethod,
  runtimeSettingsMethod, webSettingsMethod, pluginCenterMethod, pluginInstallMethod,
  questionStateMethod, questionDecisionMethod, scheduleMethod, openHostSessionMethod, sessionArchiveMethod,
  workspaceRegistryMethod,
  searchSessionsMethod, modelCatalogMethod, deepSeekAccountMethod, accountNotificationsMethod, platformSessionMethod,
  sessionReadStateMethod,
  queuePromptMethod, composerPreferenceMethod, inboxStateMethod, inboxActionMethod, goalStateMethod, goalActionMethod,
  teamStateMethod,
  terminalWatchMethod, terminalInputMethod, terminalAttachMethod, terminalDetachMethod,
  terminalCloseMethod, terminalRenameMethod,
  ...modelProviderMethods, ...messageMethods.keys(),
];

let loadingMessageFeedback;
let startingQuestions;
let sessionMutationTail = Promise.resolve();
function messageFeedback(ctx) {
  const installed = ctx.get('messageFeedback');
  if (installed) return installed;
  return loadingMessageFeedback ??= (async () => {
    const local = join(service, 'packages/feedback/message-feedback',
      profileEntry.endsWith('.ts') ? 'src/index.ts' : 'lib/index.js');
    const entry = existsSync(local) ? local : require.resolve('@deepseek-ai/dsh-message-feedback');
    const { default: MessageFeedback } = await import(pathToFileURL(entry).href);
    await ctx.plugin(MessageFeedback, { maxNoteBytes: 8192 });
    return ctx.get('messageFeedback');
  })();
}

class SessionOperationError extends Error {
  constructor(code, message = code) {
    super(message);
    this.code = code;
  }
}

/** Load the private rc.3 terminal plugins into the ACP runtime's Cordis tree. */
async function startTerminalController(ctx) {
  if (ctx.get('terminalController')) return;
  await (loadingTerminalController ??= (async () => {
    if (!process.env.KWORK_TERMINAL_MODULE) {
      throw new Error('KWork 未提供与 ACP 版本匹配的 Agent 终端模块。');
    }
    const manifest = JSON.parse(await readFile(join(dirname(process.env.KWORK_TERMINAL_MODULE), '../package.json'), 'utf8'));
    await prepareTerminalProfile({
      service,
      module: process.env.KWORK_TERMINAL_MODULE,
      home: resolveDshHome(),
      runtime: process.env.KWORK_TERMINAL_RUNTIME,
    });
    const controllerPackage = ctx.get('pluginPackages')?.packageOf(
      '@deepseek-ai/dsh-api-terminal-controller',
      pathToFileURL(join(resolveDshHome(), 'profiles/acp/package.json')).href,
    );
    if (!controllerPackage) throw new Error('ACP 未注册 Agent 终端运行时。');
    const { default: TerminalController } = await import(
      pathToFileURL(join(controllerPackage.dir, 'lib/index.js')).href,
    );
    await ctx.plugin(TerminalController, {});
    // ACP rc.1 predates these two official section ids. Supply their rc.3
    // numeric order while preserving every existing section's position.
    const prompt = ctx.get('systemPrompt');
    const sectionOrder = prompt.getSectionOrder.bind(prompt);
    prompt.getSectionOrder = name => sectionOrder(name) ??
      ({ TOOL_PTY: 1700, TOOL_PTY_PANEL: 1710 })[name];
    if (!ctx.get('terminals')) {
      const registryPackage = ctx.get('pluginPackages')?.packageOf(
        '@deepseek-ai/dsh-terminal',
        pathToFileURL(join(resolveDshHome(), 'profiles/acp/package.json')).href,
      );
      if (!registryPackage) throw new Error('ACP 未注册终端会话注册表。');
      const { TerminalSessionService } = await import(pathToFileURL(join(registryPackage.dir, 'lib/index.js')).href);
      await ctx.plugin(TerminalSessionService, {});
      const panelPackage = ctx.get('pluginPackages')?.packageOf(
        '@deepseek-ai/dsh-terminal-panel',
        pathToFileURL(join(resolveDshHome(), 'profiles/acp/package.json')).href,
      );
      if (!panelPackage) throw new Error('ACP 未注册 Agent 终端工具适配器。');
      const panel = await import(pathToFileURL(join(panelPackage.dir, 'lib/index.js')).href);
      await ctx.plugin(panel, {});
    }
    if (!terminalTools || !ctx.registry.has(terminalTools)) {
      const toolPackage = ctx.get('pluginPackages')?.packageOf(
        '@deepseek-ai/dsh-tool-terminal',
        pathToFileURL(join(resolveDshHome(), 'profiles/acp/package.json')).href,
      );
      if (!toolPackage) throw new Error('ACP 未注册模型终端工具。');
      const tools = await import(pathToFileURL(join(toolPackage.dir, 'lib/index.js')).href);
      await ctx.plugin(tools.apply ? tools : tools.default, {});
      terminalTools = tools;
    }
  })());
}

function startSessionReadStatus(ctx) {
  if (ctx.get('sessionStatus') || !ctx.get('sessions') || !ctx.get('sessionPersistence')) return;
  const status = new SessionReadStatus(ctx, record => {
    process.stdout.write(JSON.stringify({
      jsonrpc: '2.0', method: '_dsh/gpui/session-read-changed', params: record,
    }) + '\n');
  });
  ctx.provide('sessionStatus', status);
}

async function sessionReadState(ctx, params = {}) {
  const status = ctx.get('sessionStatus');
  if (params.action === 'snapshot') {
    return { available: status !== undefined, items: status?.snapshot() ?? [] };
  }
  if (!status) throw new SessionOperationError('session-status/unavailable', '当前服务不支持会话阅读状态。');
  if (typeof params.sessionId !== 'string' || !params.sessionId) {
    throw new SessionOperationError('session-status/session-id', '缺少会话 ID。');
  }
  try {
    await status.mark(params.sessionId, params.action);
  } catch (error) {
    if (typeof error?.code === 'string' && error.code.startsWith('session-status/')) {
      throw new SessionOperationError(error.code, error.message);
    }
    throw error;
  }
  return {};
}

async function startSchedule(ctx) {
  ctx.on('schedule/changed', () => {
    process.stdout.write('{"jsonrpc":"2.0","method":"_dsh/gpui/schedule-changed"}\n');
  }, { global: true });
  const sessions = ctx.get('acpSessionHost');
  ctx.on('schedule/resolve-agent', (sessionId, next) => sessions
    ? sessions.activate(sessionId)
    : ctx.get('agents')?.get(sessionId) ?? next());
  if (sessions && !ctx.get('sessionController')) {
    ctx.provide('sessionController', {
      async create({ sessionId, cwd, agentPreset }) {
        if (agentPreset !== undefined) {
          // The companion dsh-acp ACP profile does not mount the
          // agent-preset-registry service and acpSessionHost.create ignores a
          // third argument. Report that precisely instead of implying a
          // client-side limitation; silently dropping the preset would create
          // the task with the wrong agent configuration.
          throw new SessionOperationError('schedule/preset',
            '配套 dsh-acp 未在 ACP profile 中启用 Agent 预设服务，无法按指定预设创建会话。');
        }
        const { session } = await sessions.create(sessionId, cwd ?? process.env.KWORK_WORKSPACE ?? process.cwd());
        process.stdout.write(JSON.stringify({
          jsonrpc: '2.0', method: '_dsh/gpui/schedule-session', params: {
            sessionId, cwd: session.header.cwd, createdAt: session.header.createdAt,
          },
        }) + '\n');
        return { sessionId };
      },
      async resolveAgent(sessionId) { return { agent: await sessions.activate(sessionId) }; },
    });
  }
  const runtime = process.env.KWORK_SCHEDULE_MODULE;
  if (!runtime || !ctx.get('storageDomain') || !ctx.get('sessionPersistence')) return;
  const existing = ctx.get('schedule');
  const mountTools = ctx.get('sessionController') && ctx.get('tools') && ctx.get('systemPrompt') &&
    !ctx.get('tools').get('schedule_create');
  if (existing && !mountTools && ctx.get('workspaceRegistry')) {
    await importArchivedSessions(ctx);
    return;
  }
  const base = createRequire(require.resolve('@deepseek-ai/dsh-base/package.json'));
  const manager = createRequire(require.resolve('@deepseek-ai/dsh-plugin-manager'));
  const persistence = createRequire(base.resolve('@deepseek-ai/dsh-session-persistence-jsonl'));
  const schemaPackage = manager.resolve('@deepseek-ai/schemastery/package.json');
  const schemaEntry = JSON.parse(readFileSync(schemaPackage, 'utf8')).exports['.'].import;
  const entries = {
    '@deepseek-ai/schemastery': join(dirname(schemaPackage), schemaEntry),
    '@deepseek-ai/cordis': require.resolve('@deepseek-ai/cordis'),
    '@deepseek-ai/dsh-llm': base.resolve('@deepseek-ai/dsh-llm'),
    '@deepseek-ai/dsh-llm/brand': base.resolve('@deepseek-ai/dsh-llm/brand'),
    '@deepseek-ai/dsh-typert-protocol': manager.resolve('@deepseek-ai/dsh-typert-protocol'),
    '@deepseek-ai/dsh-subagent': base.resolve('@deepseek-ai/dsh-subagent'),
    '@deepseek-ai/dsh-session': base.resolve('@deepseek-ai/dsh-session'),
    '@deepseek-ai/dsh-session-persistence': persistence.resolve('@deepseek-ai/dsh-session-persistence'),
    '@deepseek-ai/dsh-storage-domain': base.resolve('@deepseek-ai/dsh-storage-domain'),
    '@deepseek-ai/dsh-tools': base.resolve('@deepseek-ai/dsh-tools'),
    zod: manager.resolve('zod'),
  };
  const source = readFileSync(runtime, 'utf8').replace(/^import (.*) from "([^"]+)";$/gm,
    (statement, bindings, name) => Object.hasOwn(entries, name)
      ? `import ${bindings} from ${JSON.stringify(pathToFileURL(entries[name]).href)};`
      : statement);
  const { default: Schedule, scheduleTools, WorkspaceRegistry } = await import('data:text/javascript;base64,' + Buffer.from(source).toString('base64'));
  if (!ctx.get('workspaceRegistry') && WorkspaceRegistry) await ctx.plugin(WorkspaceRegistry, {});
  await importArchivedSessions(ctx);
  if (!existing) await ctx.plugin(Schedule, {});
  if (mountTools) {
    try {
      await ctx.plugin(scheduleTools, {});
    } catch (error) {
      ctx.logger.warn('GPUI could not mount Schedule tools: %s', String(error));
    }
  }
}

async function importArchivedSessions(ctx) {
  const path = process.env.KWORK_WORKSPACE_PREFERENCES;
  const registry = ctx.get('workspaceRegistry');
  if (!path || !registry) return;
  const { resolveDshHome } = await import(pathToFileURL(require.resolve('@deepseek-ai/dsh-home-paths')).href);
  const marker = join(resolveDshHome(), 'kwork-archive-migration-v1.json');
  try {
    if (JSON.parse(await readFile(marker, 'utf8')).version === 1) return;
    throw new Error('Invalid KWork archive migration record');
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  let archived;
  try {
    archived = JSON.parse(await readFile(path, 'utf8')).archived ?? [];
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    archived = [];
  }
  if (!Array.isArray(archived) || archived.some(id => typeof id !== 'string' || !id)) {
    throw new Error('Invalid archived session list in KWork workspace preferences');
  }
  for (const sessionId of new Set(archived)) {
    try {
      await registry.archiveSession(sessionId);
    } catch (error) {
      if (error.name !== 'WorkspaceUnknownSessionError') throw error;
      ctx.logger.warn('KWork archive migration skipped missing Session %s', sessionId);
    }
  }
  await writeWorkspaceMigrationMarker(marker);
}

async function writeWorkspaceMigrationMarker(marker) {
  await mkdir(dirname(marker), { recursive: true });
  const temporary = marker + '.' + randomUUID() + '.tmp';
  try {
    await writeFile(temporary, JSON.stringify({ version: 1 }), { mode: 0o600 });
    await rename(temporary, marker);
  } finally {
    await rm(temporary, { force: true });
  }
}

async function readWorkspacePreferences(file) {
  try {
    return JSON.parse(await readFile(file, 'utf8'));
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    return null;
  }
}

async function importWorkspaceGroups(ctx) {
  const path = process.env.KWORK_WORKSPACE_PREFERENCES;
  const registry = ctx.get('workspaceRegistry');
  if (!path || !registry) return;
  const marker = join(resolveDshHome(), 'kwork-workspace-groups-v1.json');
  if ((await readWorkspacePreferences(marker))?.version === 1) return;
  const preferences = await readWorkspacePreferences(path);
  const paths = value => Array.isArray(value) ? value.filter(item => typeof item === 'string') : [];
  const hidden = new Set(paths(preferences?.hidden));
  const added = new Set(paths(preferences?.addedWorkspaces).filter(path => isAbsolute(path) && !hidden.has(path)));
  const ordered = new Set([...paths(preferences?.workspaceOrder).filter(path => added.has(path)), ...added]);
  // Registry.create prepends new paths and leaves the order of existing entries alone.
  for (const path of [...ordered].reverse()) {
    let info;
    try {
      info = await stat(path);
    } catch (error) {
      if (['ENOENT', 'ENOTDIR'].includes(error.code)) continue;
      throw error;
    }
    if (!info.isDirectory()) continue;
    const title = preferences?.aliases?.[path];
    await registry.create(path, typeof title === 'string' && title.trim() ? title.trim() : undefined);
  }
  await writeWorkspaceMigrationMarker(marker);
}

async function importPinnedSessions(ctx) {
  const path = process.env.KWORK_WORKSPACE_PREFERENCES;
  const registry = ctx.get('workspaceRegistry');
  if (!path || !registry) return;
  const marker = join(resolveDshHome(), 'kwork-session-pins-v1.json');
  if ((await readWorkspacePreferences(marker))?.version === 1) return;
  const pins = (await readWorkspacePreferences(path))?.pinned;
  if (Array.isArray(pins)) {
    const eligible = [...new Set(pins)].filter(id => typeof id === 'string' && id
      && !registry.pinnedSessionIds.includes(id) && !registry.archivedSessionIds.includes(id));
    // A pin is prepended. Import in reverse so an interrupted import can resume in order.
    for (const id of eligible.reverse()) {
      try {
        await registry.pinSession(id);
      } catch (error) {
        if (!['WorkspaceUnknownSessionError', 'WorkspaceArchivedSessionPinError'].includes(error.name)) throw error;
      }
    }
  }
  await writeWorkspaceMigrationMarker(marker);
}

let workspaceRegistrySequence = 0;
let workspaceMutationTail = Promise.resolve();

function enqueueWorkspaceOperation(operation) {
  const task = workspaceMutationTail.then(operation);
  workspaceMutationTail = task.then(() => undefined, () => undefined);
  return task;
}

function workspaceRegistrySnapshot(registry) {
  return {
    available: true,
    sequence: ++workspaceRegistrySequence,
    items: registry.list().map(({ id, path, title, createdAt, sessionIds }) => ({
      id, path, title, createdAt: Date.parse(createdAt), sessionIds: [...sessionIds],
    })),
    archived: [...registry.archivedSessionIds],
    pinned: [...registry.pinnedSessionIds],
  };
}

function observeWorkspaceRegistry(ctx) {
  if (!ctx.get('workspaceRegistry')) return;
  let pending = false;
  ctx.on('domain/changed', change => {
    if (pending || change.domain !== 'workspace') return;
    pending = true;
    // The official registry updates its cache after the event. A deletion
    // can also publish its pending order before the record is removed.
    setImmediate(() => {
      enqueueWorkspaceOperation(() => {
        pending = false;
        const registry = ctx.get('workspaceRegistry');
        if (registry) {
          process.stdout.write(JSON.stringify({
            jsonrpc: '2.0', method: '_dsh/gpui/workspace-registry-changed',
            params: workspaceRegistrySnapshot(registry),
          }) + '\n');
        }
      }).catch(error => {
        console.error(`KWork: 工作区状态更新失败：${error?.message ?? error}`);
      });
    });
  }, { global: true });
}

async function workspaceRegistryRequest(ctx, operation = {}) {
  const registry = ctx.get('workspaceRegistry');
  if (operation.action === 'snapshot') {
    return registry ? workspaceRegistrySnapshot(registry) : { available: false };
  }
  if (!registry) throw new SessionOperationError('workspace/unavailable', '当前服务无法管理工作区。');
  if (operation.action === 'initialize-default') {
    const workspace = await registry.initializeDefault(async () => {
      const documents = process.env.KWORK_DOCUMENTS_DIRECTORY;
      if (!documents || !isAbsolute(documents)) {
        throw new SessionOperationError('workspace/documents-unavailable', '无法读取系统文稿目录。');
      }
      return join(documents, 'deepseek-harness', 'default-workspace');
    });
    return { ...workspaceRegistrySnapshot(registry), path: workspace?.path ?? null, workspaceId: workspace?.id ?? null };
  }
  if (operation.action === 'create' && typeof operation.path === 'string' && isAbsolute(operation.path)) {
    const workspace = await registry.create(operation.path);
    return { ...workspaceRegistrySnapshot(registry), path: workspace.path };
  }
  if (operation.action === 'move' && typeof operation.id === 'string' && operation.id
      && (operation.beforeId == null || typeof operation.beforeId === 'string' && operation.beforeId)) {
    await registry.insertBefore(operation.id, operation.beforeId ?? undefined);
    return workspaceRegistrySnapshot(registry);
  }
  if (typeof operation.id === 'string' && operation.id
      && (operation.action === 'rename' || operation.action === 'delete')) {
    const workspace = registry.get(operation.id);
    if (!workspace) throw new SessionOperationError('workspace/not-found', '目标工作区已移除。');
    if (operation.action === 'delete') {
      if (!await registry.delete(workspace.id)) {
        throw new SessionOperationError('workspace/not-found', '目标工作区已移除。');
      }
    } else {
      const title = typeof operation.title === 'string' ? operation.title.trim() : '';
      if (!title) throw new SessionOperationError('workspace/invalid-name', '请输入工作区名称。');
      if (title !== workspace.title) {
        if (registry.list().some(candidate => candidate.id !== workspace.id && candidate.title === title)) {
          throw new SessionOperationError('workspace/name-conflict', `已存在名为“${title}”的工作区。`);
        }
        await workspace.setTitle(title);
      }
    }
    return workspaceRegistrySnapshot(registry);
  }
  if (typeof operation.sessionId === 'string' && operation.sessionId) {
    if (operation.action === 'attach-session' && typeof operation.id === 'string') {
      const workspace = registry.get(operation.id);
      if (!workspace) throw new SessionOperationError('workspace/not-found', '目标工作区已移除。');
      await workspace.attachSession(operation.sessionId);
      return workspaceRegistrySnapshot(registry);
    }
    if (operation.action === 'pin' || operation.action === 'unpin') {
      if (operation.action === 'pin') await registry.pinSession(operation.sessionId);
      else await registry.unpinSession(operation.sessionId);
      return workspaceRegistrySnapshot(registry);
    }
  }
  throw new SessionOperationError('workspace/action', '无效的工作区操作。');
}

async function sessionArchive(ctx, { action, sessionId, stopActivity } = {}) {
  const registry = ctx.get('workspaceRegistry');
  if (!registry) {
    if (action === 'read') return { available: false };
    throw new SessionOperationError('workspace/unavailable', '当前服务无法归档会话。');
  }
  if (action !== 'read') {
    if (typeof sessionId !== 'string' || !sessionId) {
      throw new SessionOperationError('workspace/session-id', '缺少会话 ID。');
    }
    if (action === 'archive') {
      try {
        await registry.archiveSession(sessionId, { stopActivity: stopActivity === true });
      } catch (error) {
        if (error.name === 'WorkspaceActiveSessionError') return { activity: error.activity };
        throw error;
      }
    } else if (action === 'restore') {
      await registry.unarchiveSession(sessionId);
    } else {
      throw new SessionOperationError('workspace/action', '找不到这个会话操作。');
    }
  }
  return workspaceRegistrySnapshot(registry);
}

async function scheduleRequest(ctx, operation) {
  const schedule = ctx.get('schedule');
  if (operation?.action === 'catalog') {
    return schedule ? {
      available: true,
      tasks: await schedule.catalog(),
      limits: schedule.deliveryLimits(),
      canCreateSession: ctx.get('sessionController') !== undefined,
      conversationTools: ctx.get('tools')?.get('schedule_create') !== undefined,
    } : { available: false };
  }
  if (!schedule) throw new SessionOperationError('schedule/unavailable', '当前 ACP 服务无法管理自动化任务。');
  try {
    switch (operation?.action) {
      case 'create': return await schedule.remoteCreate(operation.request);
      case 'edit': return await schedule.remoteEdit({ expectedUpdatedAt: null, ...operation.request });
      case 'run': return await schedule.run({ id: operation.id });
      case 'history': return await schedule.remoteHistory(operation.request);
      case 'clearEnded': return await schedule.deleteInactive();
      default: throw new SessionOperationError('schedule/action', '找不到这个自动化操作。');
    }
  } catch (error) {
    if (typeof error?.code === 'string' && error.code.startsWith('schedule/')) {
      throw new SessionOperationError(error.code, error.message);
    }
    throw error;
  }
}

const runtimeSections = {
  shell: { namespaces: ['pwsh-sandbox', 'bash-sandbox'], fields: ['timeoutMs', 'maxOutputBytes'] },
  agentLoop: { namespaces: ['agent-loop'], fields: ['maxParallelToolCalls'] },
  subagent: { namespaces: ['subagent'], fields: ['maxDepth', 'maxActiveSubagents'] },
};

function runtimeSettings(ctx, { page, change } = {}) {
  if (!Object.hasOwn(runtimeSections, page)) {
    throw new SessionOperationError('settings/page', '找不到这个设置页面。');
  }
  const { namespaces, fields } = runtimeSections[page];
  const settings = ctx.get('settings');
  const read = () => {
    const entries = settings?.describe({ redactSecrets: true }) ?? [];
    const entry = namespaces.map(ns => entries.find(row => row.ns === ns)).find(Boolean);
    if (!entry || !fields.every(field => typeof entry.value[field] === 'number')) return null;
    return {
      namespace: entry.ns, revision: entry.revision, writable: settings.writable,
      fields: Object.fromEntries(fields.map(field => [field, {
        value: entry.value[field],
        base: entry.base[field],
        overridden: Object.hasOwn(entry.user, field),
      }])),
    };
  };
  if (change == null) return runtimeSettingsTail.then(read);

  const operation = runtimeSettingsTail.then(async () => {
    const current = read();
    if (!current?.writable) {
      throw new SessionOperationError('settings/read-only', '当前部署无法保存这项设置。');
    }
    if (change.namespace !== current.namespace || change.revision !== current.revision) {
      throw new SessionOperationError('settings/conflict', '设置已更新，请重新打开页面后再试。');
    }
    if (!change.fields || typeof change.fields !== 'object' || Array.isArray(change.fields)) {
      throw new SessionOperationError('settings/fields', '设置内容无效。');
    }
    const ops = Object.entries(change.fields).map(([field, edit]) => {
      if (!fields.includes(field)) {
        throw new SessionOperationError('settings/value', '该字段无法在此页面编辑。');
      }
      if (edit?.op === 'unset') return { op: 'unset', path: [field] };
      const value = edit?.value;
      const valid = page === 'shell'
        ? Number.isFinite(value) && value > 0
        : Number.isSafeInteger(value) && value >= (field === 'maxDepth' ? 0 : 1);
      if (edit?.op !== 'set' || !valid) {
        throw new SessionOperationError('settings/value', '请输入此字段允许的数字，或留空以恢复默认值。');
      }
      return { op: 'set', path: [field], value };
    });
    if (ops.length) {
      try { await settings.mutate(current.namespace, ops, current.revision); }
      catch { throw new SessionOperationError('settings/save', '保存失败。运行时未接受这些值，已保留供你修改。'); }
    }
    return read();
  });
  runtimeSettingsTail = operation.then(() => undefined, () => undefined);
  return operation;
}

let runtimeSettingsTail = Promise.resolve();

// Older runtimes may not export the presentation sets. Keep profile layers
// out of both the cards and the mutation endpoint on those runtimes as well.
const hiddenProfileBundles = new Set([
  'base', 'web-app', 'headless', 'sdk-app', 'acp-app', 'sdk-minimal', 'ui-app',
  'experimental-terminal-bundle', 'experimental-tool-worktree',
].map(name => '@deepseek-ai/dsh-' + name));
let pluginCatalog;
function pluginPresentation() {
  return pluginCatalog ??= (async () => {
    try {
      return await import(pathToFileURL(require.resolve('@deepseek-ai/dsh-official-bundles')).href);
    } catch {
      return {};
    }
  })();
}

async function pluginCenter(ctx) {
  const manager = ctx.get('pluginManager');
  const namespaces = new Set((ctx.get('settings')?.describe({ redactSecrets: true }) ?? [])
    .map(entry => entry.ns));
  const configs = Object.entries(runtimeSections)
    .filter(([, section]) => section.namespaces.some(ns => namespaces.has(ns)))
    .map(([page]) => page);
  if (namespaces.has('web') && ctx.get('web')) configs.push('webSearch');
  if (!manager) return { configs, entries: [], bundles: [], managementAvailable: false };

  const [entries, bundles, catalog] = await Promise.all([
    manager.listPlugins(), manager.listBundles(), pluginPresentation(),
  ]);
  const hidden = name => hiddenProfileBundles.has(name) || catalog.HIDDEN_BUNDLES?.has(name);
  const live = new Set(entries.filter(entry => entry.enabled).map(entry => entry.entryId));
  const mainOrder = catalog.EXTENSION_BUNDLES ?? [];
  const moreOrder = catalog.MORE_BUNDLES ?? [];
  return {
    configs,
    managementAvailable: true,
    entries: entries.map(({ entryId, moduleName, enabled, fiberPhase, diagnostic, readOnlyReason, meta }) => ({
      entryId, moduleName, enabled, fiberPhase, diagnostic, readOnlyReason,
      title: localizedText(meta?.title), description: localizedText(meta?.description),
    })),
    bundles: bundles.filter(bundle => !hidden(bundle.name) &&
      (bundle.installed || bundle.official || bundle.error !== undefined) &&
      (bundle.listedByDefault !== false || bundle.enabled || bundle.installed ||
        bundle.rows.some(row => live.has(row.entryId)))).map(bundle => {
      const { name, version, enabled, installed, official, removable, availability,
        readOnlyReason, error, meta, rows, installTarget, updatable } = bundle;
      const first = mainOrder.indexOf(name);
      const last = moreOrder.indexOf(name);
      return {
        name, version, enabled, installed, official, removable, availability, readOnlyReason, error: error?.code,
        experimental: catalog.EXPERIMENTAL_BUNDLES?.has(name) ?? false,
        catalogVersion: installTarget?.version,
        catalogUpdate: catalogUpdateAvailable(bundle),
        updatable: updatable === true && typeof manager.listVersions === 'function' &&
          typeof manager.updateBundle === 'function',
        moreOnly: last >= 0 && !enabled,
        rank: first >= 0 ? first : last >= 0 ? mainOrder.length + 1 + last : mainOrder.length,
        title: localizedText(meta?.title),
        description: localizedText(meta?.description) ?? bundle.description,
        rows: rows.map(({ rowId, moduleName, entryId, meta }) => ({
          rowId, moduleName, entryId,
          title: localizedText(meta?.title), description: localizedText(meta?.description),
        })),
      };
    }),
  };
}

let pluginCenterTail = Promise.resolve();

function pluginCenterRequest(ctx, { operation } = {}) {
  if (operation == null) return pluginCenterTail.then(() => pluginCenter(ctx));
  const task = pluginCenterTail.then(async () => {
    const snapshot = await pluginCenter(ctx);
    const manager = ctx.get('pluginManager');
    const bundle = snapshot.bundles.find(item => item.name === operation.name);
    const fail = (error) => ({ snapshot, outcome: { application: 'failed', error } });
    if (pluginInstallJob?.running) return fail('install-running');
    if (!manager || !bundle) return fail('unknown-bundle');
    let result;
    switch (operation.kind) {
      case 'bundle':
        if (typeof operation.enabled !== 'boolean') return fail('invalid-operation');
        if (bundle.readOnlyReason) return fail(bundle.readOnlyReason);
        if (operation.enabled && bundle.availability === 'missing') return fail('missing-bundle');
        if (operation.enabled && bundle.error) return fail(bundle.error);
        result = await manager.setBundleEnabled(bundle.name, operation.enabled);
        break;
      case 'entry': {
        if (typeof operation.enabled !== 'boolean') return fail('invalid-operation');
        const row = bundle.rows.find(item => item.rowId === operation.rowId);
        const entry = row?.entryId && snapshot.entries.find(item => item.entryId === row.entryId);
        if (!bundle.enabled || !entry) return fail('unknown-plugin');
        if (entry.readOnlyReason) return fail(entry.readOnlyReason);
        result = await manager.setPluginEnabled(entry.entryId, operation.enabled);
        break;
      }
      case 'remove':
        if (bundle.readOnlyReason || !bundle.removable) return fail(bundle.readOnlyReason ?? 'not-removable');
        result = await manager.removeBundle(bundle.name);
        break;
      default: return fail('invalid-operation');
    }
    // A profile write can succeed even if re-reading the inventory fails.
    // Keep the Host outcome separate from the refreshed presentation.
    return {
      outcome: {
        application: result.application,
        changed: result.changed,
        restartReason: result.restartReason,
        error: result.error?.code,
      },
      snapshot: await pluginCenter(ctx).catch(() => null),
    };
  });
  pluginCenterTail = task.then(() => undefined, () => undefined);
  return task;
}

let pluginInstallJob;
const installRequestId = /^[0-9a-f]{32}$/;

function pluginInstallEvent(requestId, event) {
  process.stdout.write(JSON.stringify({
    jsonrpc: '2.0', method: '_dsh/gpui/plugin-install-event', params: { requestId, ...event },
  }) + '\n');
}

function observePluginInstall(ctx) {
  ctx.on('plugin-manager/install-state', ({ requestId, phase, attempt }) => {
    if (requestId === pluginInstallJob?.id && pluginInstallJob.running) {
      pluginInstallEvent(requestId, { type: 'phase', phase, attempt });
    }
  }, { global: true });
  ctx.on('plugin-manager/update-state', ({ requestId, step }) => {
    if (requestId === pluginInstallJob?.id && pluginInstallJob.running) {
      pluginInstallEvent(requestId, { type: 'update', step });
    }
  }, { global: true });
  ctx.on('plugin-manager/install-log', ({ requestId, jobId, stream, text, exitCode }) => {
    if (requestId === pluginInstallJob?.id && pluginInstallJob.running) {
      // Install logs are for the main UI only. Do not forward argv, which can
      // contain a user-supplied URL, or the local profile directory.
      pluginInstallEvent(requestId, {
        type: 'log', jobId, stream, text: stripVTControlCharacters(text), exitCode,
      });
    }
  }, { global: true });
}

function safePluginRegistry(value) {
  if (value == null) return value;
  const url = typeof value === 'string' && URL.canParse(value) ? new URL(value) : null;
  if (!url || !['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.hash) {
    throw new SessionOperationError('plugins/registry', '请输入有效的 HTTP 或 HTTPS 安装源地址。');
  }
  return url.href;
}

function installSummary(result) {
  return {
    kind: 'settled',
    application: result.application,
    stage: result.stage,
    changed: result.changed,
    bundle: result.bundle,
    version: result.version,
    enabled: result.enabled,
    restartReason: result.restartReason,
    problem: result.error?.code,
    failure: result.packageResult?.kind,
    pendingBuilds: result.pendingBuilds,
  };
}

function catalogUpdateAvailable(bundle) {
  return bundle.official && bundle.installed && bundle.removable && !bundle.readOnlyReason &&
    bundle.availability === 'profile' && bundle.installTarget !== undefined &&
    !matchesCatalogTarget(bundle, bundle.installTarget);
}

function matchesCatalogTarget(bundle, target) {
  return bundle.version === target.version &&
    ((!target.spec.startsWith('link:') && !bundle.source?.startsWith('link:')) || bundle.source === target.spec);
}

async function updateSettlement(manager, name, from, to, result, restore, catalog) {
  let observed;
  let inventoryRead = false;
  try {
    observed = (await manager.listBundles()).find(bundle => bundle.name === name);
    inventoryRead = true;
  } catch { /* The Host result still describes the attempted change. */ }
  const restored = restore && ['applied', 'restart-required'].includes(restore.application);
  const changed = observed?.version !== from || (catalog && catalog.previous !== observed?.source);
  const recovery = !inventoryRead ? 'unknown' : observed?.installed !== true ? 'missing' :
    changed ? 'changed' : restored ? 'restored' : 'kept';
  const installed = observed?.installed === true &&
    (result.application === 'failed' || !catalog || matchesCatalogTarget(observed, catalog.target));
  return {
    ...installSummary(result),
    bundle: installed ? name : undefined,
    version: installed ? observed.version : undefined,
    enabled: installed ? observed.enabled : undefined,
    update: { from, to, recovery },
  };
}

async function runPluginVersionUpdate(manager, job, input) {
  const { name, from, version, registry } = input;
  const bundle = (await manager.listBundles()).find(bundle => bundle.name === name);
  if (bundle?.updatable !== true || bundle.version !== from || bundle.readOnlyReason ||
      typeof manager.updateBundle !== 'function') {
    return { kind: 'refused', problem: 'stale-version' };
  }
  pluginInstallEvent(job.id, { type: 'subject', subject: {
    name, version, description: localizedText(bundle.meta?.description) ?? bundle.description,
    source: 'registry',
  } });
  job.started = true;
  const result = await manager.updateBundle(name, version, { registry, requestId: job.id });
  return updateSettlement(manager, name, result.from || from, result.to, result.result, result.restore);
}

async function runCatalogInstall(ctx, manager, job, input, approvedBuilds) {
  const { name, from, version, registry } = input;
  const bundle = (await manager.listBundles()).find(bundle => bundle.name === name);
  const target = bundle?.installTarget;
  if (!bundle?.official || !target || target.version !== version || bundle.readOnlyReason ||
      (from === null
        ? bundle.installed || bundle.availability !== 'missing' || bundle.error
        : (bundle.version ?? '') !== from || !catalogUpdateAvailable(bundle))) {
    return { kind: 'refused', problem: 'stale-version' };
  }
  if (job.controller.signal.aborted) return { kind: 'cancelled' };
  const enabled = from === null ? input.enabled : bundle.enabled;
  const local = target.spec.startsWith('link:');
  const subject = {
    name, version, description: localizedText(bundle.meta?.description) ?? bundle.description,
    source: local ? 'path' : 'registry',
  };
  const replaceLive = from !== null && ctx.get('hmr') !== undefined;
  const source = bundle.source;
  // HMR updates stop and remove the running package first. If the update fails
  // after removal, reinstall the exact previous release or its saved source.
  if (replaceLive && !source) return { kind: 'refused', problem: 'not-updatable' };
  job.input = input;
  pluginInstallEvent(job.id, { type: 'subject', subject });
  job.started = true;
  if (replaceLive) {
    pluginInstallEvent(job.id, { type: 'update', step: 'removing' });
    const removal = await manager.removeBundle(name).catch(() => ({
      application: 'failed', stage: 'remove', changed: false, error: { code: 'operation-error' },
    }));
    if (removal.application === 'failed' || removal.application === 'cancelled') {
      return updateSettlement(manager, name, from, version, removal);
    }
  }
  if (from !== null) pluginInstallEvent(job.id, { type: 'update', step: 'installing' });
  let result;
  try {
    result = await manager.installBundle(target.spec, {
      requestId: job.id, registry, enabled, saveExact: !local,
      ...(approvedBuilds === undefined ? {} : { approvedBuilds }),
    });
  } catch (error) {
    if (!replaceLive) throw error;
    result = { application: 'failed', stage: 'install', changed: false, error: { code: 'operation-error' } };
  }
  if (from === null) return installSummary(result);
  let restore;
  if (replaceLive && (result.application === 'failed' || result.application === 'cancelled')) {
    pluginInstallEvent(job.id, { type: 'update', step: 'restoring' });
    if (result.stage === 'enable') await manager.removeBundle(name).catch(() => undefined);
    const range = source.startsWith(name + '@') ? source.slice(name.length + 1) : null;
    const previous = from && range !== null && !range.includes(':') ? name + '@' + from : source;
    restore = await manager.installBundle(previous, { enabled, saveExact: true }).catch(() => null);
  }
  return updateSettlement(manager, name, from, version, result, restore, { target, previous: source });
}

async function runPluginInstall(ctx, manager, job, input, approvedBuilds) {
  await pluginCenterTail;
  if (job.controller.signal.aborted) return { kind: 'cancelled' };
  if (input.kind === 'update') return runPluginVersionUpdate(manager, job, input);
  if (input.kind === 'catalog') return runCatalogInstall(ctx, manager, job, input, approvedBuilds);
  const { spec, registry } = input;
  let subject = input.subject;
  if (!subject) {
    const catalog = (await manager.listBundles()).find(bundle =>
      bundle.name === spec && bundle.official && bundle.availability === 'missing' && bundle.installTarget);
    if (catalog) return runCatalogInstall(ctx, manager, job, {
      kind: 'catalog', name: spec, version: catalog.installTarget.version, from: null, enabled: false, registry,
    });
    if (job.controller.signal.aborted) return { kind: 'cancelled' };
    const checked = await manager.inspect(spec, { registry }, job.controller.signal);
    if (job.controller.signal.aborted) return { kind: 'cancelled' };
    if (checked.status !== 'accepted') return { kind: 'refused', problem: checked.problem };
    subject = {
      name: checked.name, version: checked.version, description: checked.description,
      registry: checked.registry, source: checked.kind,
    };
  }
  job.input = { spec, registry, subject };
  pluginInstallEvent(job.id, { type: 'subject', subject });
  job.started = true;
  const result = await manager.installBundle(spec, {
    requestId: job.id, registry: subject.registry, enabled: false,
    ...(approvedBuilds === undefined ? {} : { approvedBuilds }),
  });
  return installSummary(result);
}

async function pluginInstallRequest(ctx, operation) {
  const manager = ctx.get('pluginManager');
  if (!manager) throw new SessionOperationError('plugins/unavailable', '当前 ACP 服务无法安装插件。');
  if (operation?.action === 'registries') return manager.registries();
  if (operation?.action === 'versions') {
    if (pluginInstallJob?.running || typeof manager.listVersions !== 'function') {
      return { status: 'refused', problem: 'not-updatable', attempts: [] };
    }
    return manager.listVersions(operation.name, {});
  }
  const id = operation?.requestId;
  if (typeof id !== 'string' || !installRequestId.test(id)) {
    throw new SessionOperationError('plugins/request', '插件安装请求无效。');
  }
  if (operation.action === 'observe') {
    return pluginInstallJob?.id === id ? pluginInstallJob.promise : null;
  }
  if (operation.action === 'cancel') {
    const job = pluginInstallJob;
    if (job?.id !== id || !job.running) return { status: 'not-running' };
    if (job.nonCancellable) return { status: 'too-late' };
    if (job.started) return manager.cancelInstall(id);
    job.controller.abort();
    await job.promise;
    return { status: 'cancelled' };
  }
  if (['run', 'approve', 'catalog', 'update'].includes(operation.action) && pluginInstallJob?.id === id) {
    return pluginInstallJob.promise;
  }
  if (pluginInstallJob?.running) {
    throw new SessionOperationError('plugins/busy', '另一个插件安装仍在进行。');
  }
  let input;
  let approvedBuilds;
  if (operation.action === 'approve') {
    const previous = pluginInstallJob;
    approvedBuilds = previous?.result?.pendingBuilds;
    if (!previous || previous.id !== operation.previousRequestId || id === previous.id ||
        !previous.input || previous.result?.application !== 'failed' ||
        !Array.isArray(approvedBuilds) || approvedBuilds.length === 0) {
      throw new SessionOperationError('plugins/approval', '安装脚本的授权已失效，请重新检查插件。');
    }
    input = previous.input;
  } else if (operation.action === 'catalog' || operation.action === 'update') {
    const { name, from, version, enabled, registry } = operation;
    if (typeof name !== 'string' || typeof version !== 'string' || (from !== null && typeof from !== 'string') ||
        (operation.action === 'catalog' ? typeof enabled !== 'boolean' : from === null)) {
      return { kind: 'refused', problem: 'invalid-spec' };
    }
    input = { kind: operation.action, name, from, version, enabled, registry: safePluginRegistry(registry) };
  } else if (operation.action === 'run') {
    const spec = typeof operation.spec === 'string' ? operation.spec.trim() : '';
    if (!spec || spec.length > 4096 || spec.startsWith('-') || /[\x00-\x1f\x7f]/u.test(spec)) {
      return { kind: 'refused', problem: 'invalid-spec' };
    }
    try {
      input = { spec, registry: safePluginRegistry(operation.registry) };
    } catch (error) {
      if (error instanceof SessionOperationError) return { kind: 'refused', problem: 'invalid-registry' };
      throw error;
    }
  } else {
    throw new SessionOperationError('plugins/request', '插件安装请求无效。');
  }
  const nonCancellable = input.kind === 'update' || (input.kind === 'catalog' && input.from !== null);
  const job = { id, running: true, started: false, nonCancellable, controller: new AbortController() };
  pluginInstallJob = job;
  job.promise = runPluginInstall(ctx, manager, job, input, approvedBuilds).catch(() => {
    if (job.controller.signal.aborted) return { kind: 'cancelled' };
    return job.started ? { kind: 'unknown' } : { kind: 'refused', problem: 'unknown' };
  }).then(result => {
    job.result = result;
    job.running = false;
    return result;
  });
  return job.promise;
}

const webFieldSections = {
  count: 'web-deepseek-api',
  baseURL: 'web-search-deepseek',
  maxUses: 'web-search-deepseek',
};
const webSections = ['web-deepseek-api', 'web-search-deepseek', 'web'];
const legacySearch = 'deepseek-official';
let webSettingsTail = Promise.resolve();

function webSettingsError(message) {
  return new SessionOperationError('settings/web', message);
}

function webSettingsEntries(settings) {
  return new Map((settings?.describe({ redactSecrets: true }) ?? [])
    .filter(entry => webSections.includes(entry.ns)).map(entry => [entry.ns, entry]));
}

function legacyCredentialRef(entries) {
  return entries.get('web-search-deepseek')?.value.apiKeyEnv || 'DEEPSEEK_API_KEY';
}

function localizedText(text) {
  return typeof text === 'string' ? text : text?.zh ?? text?.en;
}

async function webSettingsSnapshot(ctx) {
  const settings = ctx.get('settings');
  const entries = webSettingsEntries(settings);
  const web = entries.get('web');
  const runtime = ctx.get('web');
  if (!web || !runtime) return null;
  let providers;
  try { providers = runtime.listProviders(); } catch { /* Keep the current selection visible. */ }
  const selection = capability => {
    const key = capability + 'Provider';
    return {
      value: web.value[key], base: web.base[key],
      options: providers?.[capability].map(({ id, title, description }) => ({
        id, title: localizedText(title), description: localizedText(description),
      })) ?? [],
    };
  };
  const fields = {};
  for (const [name, namespace] of Object.entries(webFieldSections)) {
    const entry = entries.get(namespace);
    if (entry) fields[name] = {
      value: entry.value[name], base: entry.base[name], overridden: Object.hasOwn(entry.user, name),
    };
  }
  let credential;
  if (entries.has('web-search-deepseek')) {
    const ref = legacyCredentialRef(entries);
    try {
      const info = await ctx.get('credentials')?.describe(ref);
      if (info) credential = { ref, configured: info.configured, writable: info.writable };
    } catch { /* Invalid or unavailable references cannot be edited here. */ }
  }
  const billing = capability => {
    const field = entries.get('web-deepseek-api')?.value[capability + 'Billing'];
    if (!['auto', 'account', 'api-key'].includes(field)) return;
    const advancedOverride = [`${capability}BaseURL`, `${capability}ApiKeyEnv`]
      .filter(name => entries.get('web-deepseek-api')?.value[name])
      .map(name => name === `${capability}BaseURL` ? 'Base URL' : 'API Key 引用');
    return { value: field, advancedOverride };
  };
  return {
    search: selection('search'), fetch: selection('fetch'), fields, credential,
    searchBilling: billing('search'), fetchBilling: billing('fetch'),
    writable: settings.writable, providersFailed: !providers,
    revisions: Object.fromEntries([...entries].map(([ns, entry]) => [ns, entry.revision])),
  };
}

function webFieldEdit(name, edit) {
  if (edit?.op === 'unset') return { op: 'unset', path: [name] };
  const value = edit?.value;
  if (edit?.op !== 'set') throw webSettingsError('设置内容无效。');
  if (name === 'baseURL') {
    const url = typeof value === 'string' && URL.canParse(value) ? new URL(value) : undefined;
    if (!url || !['http:', 'https:'].includes(url.protocol) || url.username || url.password) {
      throw webSettingsError('请输入有效的 HTTP 或 HTTPS 地址。');
    }
  } else if (!Number.isSafeInteger(value) || value < 1 || (name === 'count' && value > 50)) {
    throw webSettingsError('搜索结果数须为 1–50 的整数，单次请求的搜索上限须为正整数。');
  }
  return { op: 'set', path: [name], value };
}

async function saveWebSettings(ctx, change) {
  const settings = ctx.get('settings');
  const entries = webSettingsEntries(settings);
  const web = entries.get('web');
  const runtime = ctx.get('web');
  if (!web || !runtime) throw webSettingsError('该插件当前未加载，暂时无法配置。');
  const sections = new Map();
  const add = (namespace, edit) => {
    if (!sections.has(namespace)) sections.set(namespace, []);
    sections.get(namespace).push(edit);
  };
  let search = web.value.searchProvider;
  if (change.search !== undefined || change.fetch !== undefined) {
    const providers = runtime.listProviders();
    for (const capability of ['search', 'fetch']) {
      const id = change[capability];
      if (id === undefined) continue;
      const name = capability + 'Provider';
      const base = web.base[name];
      if (typeof id !== 'string' || (id === '' ? base !== undefined :
          !providers[capability].some(provider => provider.id === id))) {
        throw webSettingsError('所选提供方已不可用，请重新打开页面后再试。');
      }
      if (capability === 'search') search = id || undefined;
      add('web', id === '' || id === base ? { op: 'unset', path: [name] } :
        { op: 'set', path: [name], value: id });
    }
  }
  for (const capability of ['search', 'fetch']) {
    const value = change[capability + 'Billing'];
    if (value === undefined) continue;
    if (!['auto', 'account', 'api-key'].includes(value)) {
      throw webSettingsError('计费方式无效，请重新选择。');
    }
    const api = entries.get('web-deepseek-api');
    const provider = capability === 'search' ? search : change.fetch ?? web.value.fetchProvider;
    if (provider !== `deepseek-${capability}-api` || !api || api.value[capability + 'Billing'] === undefined ||
        api.value[capability + 'BaseURL'] || api.value[capability + 'ApiKeyEnv']) {
      throw webSettingsError('该提供方的计费方式当前不可编辑。');
    }
    add('web-deepseek-api', value === api.base[capability + 'Billing']
      ? { op: 'unset', path: [capability + 'Billing'] }
      : { op: 'set', path: [capability + 'Billing'], value });
  }
  for (const [name, edit] of Object.entries(change.fields ?? {})) {
    if (!Object.hasOwn(webFieldSections, name)) throw webSettingsError('该字段无法在此页面编辑。');
    add(webFieldSections[name], webFieldEdit(name, edit));
  }
  const credentials = ctx.get('credentials');
  const credential = change.credential;
  if (credential !== undefined) {
    if (search !== legacySearch || !entries.has('web-search-deepseek') || !credentials ||
        credential?.ref !== legacyCredentialRef(entries) || typeof credential.key !== 'string' ||
        !credential.key.trim() || !(await credentials.describe(credential.ref)).writable) {
      throw webSettingsError('当前提供方的 API Key 无法保存。');
    }
  }
  if (sections.size && !settings.writable) throw webSettingsError('本部署的设置为只读。');
  for (const namespace of sections.keys()) {
    const entry = entries.get(namespace);
    if (!entry || entry.revision !== change.revisions?.[namespace]) {
      throw webSettingsError('设置已更新，请重新打开页面后再试。');
    }
  }

  const applied = [];
  let failure;
  try {
    for (const namespace of webSections) {
      const edits = sections.get(namespace);
      if (!edits) continue;
      await settings.mutate(namespace, edits, entries.get(namespace).revision);
      applied.push(...edits.map(edit => edit.path[0]));
    }
    if (credential !== undefined) {
      // Revalidate the reference after settings writes. A draft must never be
      // redirected to a different reference while saving.
      const current = webSettingsEntries(settings);
      if (current.get('web')?.value.searchProvider !== legacySearch ||
          credential.ref !== legacyCredentialRef(current)) {
        throw webSettingsError('提供方或 API Key 引用已更新。');
      }
      await credentials.set(credential.ref, credential.key.trim());
      applied.push('credential');
    }
  } catch {
    failure = '本部署没有接受全部设置，未保存的内容已保留供你修改。';
  }
  return { snapshot: await webSettingsSnapshot(ctx), applied, failure };
}

function webSettings(ctx, { change } = {}) {
  if (change == null) return webSettingsTail.then(async () => ({ snapshot: await webSettingsSnapshot(ctx), applied: [] }));
  const operation = webSettingsTail.then(() => saveWebSettings(ctx, change));
  webSettingsTail = operation.then(() => undefined, () => undefined);
  return operation;
}

async function checkpointTitle(ctx, session) {
  await ctx.get('sessionProjectionCache')?.write(session).catch(error => {
    ctx.logger.warn('Could not checkpoint the session title: %s', String(error));
  });
}

async function renameSession(ctx, { sessionId, title }) {
  if (typeof title !== 'string') {
    throw new SessionOperationError('session/title-invalid', 'session title must contain visible characters');
  }
  const sessions = ctx.get('sessions');
  const titles = ctx.get('sessionTitle');
  let session = sessions.get(sessionId);
  let handle;
  if (!session) {
    const stored = await ctx.get('sessionPersistence').stat(sessionId);
    session = sessions.get(sessionId);
    if (!session) {
      if (stored?.header.cwd === undefined) throw new SessionOperationError('session/not-found');
      if (stored.header.origin === 'subagent') throw new SessionOperationError('session/agent-busy');
      try {
        const { provider, model } = ctx.get('agentDefaultModel').currentSelection();
        handle = await ctx.get('agents').resume({
          resumeSessionId: sessionId,
          agentOptions: { provider, model },
        });
        session = handle.agent.session;
      } catch (error) {
        session = sessions.get(sessionId);
        if (!session) {
          if (error?.name === 'SessionAlreadyOwnedError') {
            throw new SessionOperationError('session/writer-held', error.message);
          }
          throw error;
        }
      }
    }
  }
  try {
    if (session.header.origin === 'subagent') throw new SessionOperationError('session/agent-busy');
    const accepted = titles.rename(session, title);
    await sessions.flush(session);
    await checkpointTitle(ctx, session);
    return { title: accepted.title, seq: accepted.eventSeq };
  } catch (error) {
    if (error?.name === 'SessionTitleInvalidError') {
      throw new SessionOperationError('session/title-invalid', error.message);
    }
    throw error;
  } finally {
    await handle?.dispose();
  }
}

function latestCompletedBoundary(events) {
  const end = events.findLast(event => event.type === 'turn/end');
  if (!end) return;
  let boundary = end.seq;
  for (const next of events.slice(boundary + 1)) {
    if (next.type === 'turn/start' || next.type === 'agent/inbox/spliced'
        || (next.type === 'user/message' && next.surfaceOp === 'append')) break;
    boundary = next.seq;
  }
  return boundary;
}

function listedTitle(ctx, header) {
  const live = ctx.get('sessions').get(header.id);
  if (live) return ctx.get('sessionTitle')?.get(live)?.title;
  const cache = ctx.get('sessionProjectionCache');
  return cache?.cachedSnapshot(header, ['title'])?.values.title
    ?? cache?.cachedPredecessorTitle(header)?.values.title;
}

function nextTitle(title) {
  const ascii = /^(.*?)\((\d+)\)$/u.exec(title);
  if (ascii) return `${ascii[1]}(${BigInt(ascii[2]) + 1n})`;
  const full = /^(.*?)（(\d+)）$/u.exec(title);
  if (full) return `${full[1]}（${BigInt(full[2]) + 1n}）`;
  return `${title} (1)`;
}

async function forkSession(ctx, { sessionId, atSeq, allowMigration }) {
  if (atSeq !== undefined && (!Number.isSafeInteger(atSeq) || atSeq < 0)) {
    throw new SessionOperationError('session/fork-unavailable');
  }
  if (allowMigration === false && !ctx.get('sessions').get(sessionId)) {
    const stored = await ctx.get('sessionPersistence').stat(sessionId);
    if (stored?.formatStatus === 'migration-required' && !ctx.get('sessions').get(sessionId)) {
      throw new SessionOperationError('session/migration-required');
    }
  }
  const query = ctx.get('sessionQuery');
  let source;
  try {
    source = await query.observeSession(sessionId, { projectionMode: 'none' });
  } catch (error) {
    if (error?.code === 'SESSION_QUERY_SESSION_NOT_FOUND') throw new SessionOperationError('session/not-found');
    throw error;
  }
  try {
    const { header, events } = source;
    const boundary = atSeq ?? latestCompletedBoundary(events);
    if (boundary === undefined || events[boundary]?.seq !== boundary) {
      throw new SessionOperationError('session/fork-unavailable');
    }
    const registry = ctx.get('workspaceRegistry');
    const workspaces = await enqueueWorkspaceOperation(() => registry?.list() ?? []);
    let workspace = workspaces.find(candidate => candidate.sessionIds.includes(sessionId));
    if (!workspace && workspaces.length > 0 && header.origin === 'subagent') {
      const lineage = await query.traceSession(sessionId);
      for (const ancestor of lineage.ancestors) {
        workspace = workspaces.find(candidate => candidate.sessionIds.includes(ancestor.header.id));
        if (workspace) break;
      }
    }
    const local = join(service, 'packages/core/session', profileEntry.endsWith('.ts') ? 'src/fork.ts' : 'lib/fork.js');
    const entry = existsSync(local) ? local : require.resolve('@deepseek-ai/dsh-session/fork');
    const { buildForkSeed } = await import(pathToFileURL(entry).href);
    const sourceTitle = listedTitle(ctx, header);
    let title;
    if (sourceTitle !== undefined && sourceTitle !== null) {
      const records = await query.listSessions();
      const taken = new Set(records.map(record => listedTitle(ctx, record.header)));
      title = nextTitle(sourceTitle);
      while (taken.has(title)) title = nextTitle(title);
    }
    const id = `session-${randomUUID()}`;
    const { provider, model } = ctx.get('agentDefaultModel').currentSelection();
    const handle = await ctx.get('agents').create({
      sessionId: id,
      seed: buildForkSeed(events, boundary),
      inheritedEventCount: boundary + 1,
      meta: {
        isSeeded: true,
        ...(header.cwd === undefined ? {} : { cwd: header.cwd }),
        ...(header.agentPreset === undefined ? {} : { agentPreset: header.agentPreset }),
      },
      agentOptions: { provider, model },
    });
    try {
      const child = handle.agent.session;
      if (title !== undefined) {
        title = ctx.get('sessionTitle').rename(child, title).title;
        // The sidebar can fork again immediately after this child goes cold.
        await checkpointTitle(ctx, child);
      }
      const result = { sessionId: id, cwd: child.header.cwd, createdAt: child.header.createdAt, title };
      if (workspace) {
        try {
          result.workspaceRegistry = await enqueueWorkspaceOperation(() =>
            workspaceRegistryRequest(ctx, { action: 'attach-session', id: workspace.id, sessionId: id }));
        } catch (error) {
          result.workspaceAttachError = error instanceof Error ? error.message : String(error);
        }
      }
      return result;
    } finally {
      // Persist the prefix and release the write lease before ACP resumes the child.
      await handle.dispose();
    }
  } finally {
    source[Symbol.dispose]();
  }
}

function notify(sessionId, update) {
  process.stdout.write(JSON.stringify({
    jsonrpc: '2.0', method: 'session/update', params: { sessionId, update },
  }) + '\n');
}

/** Flatten a message's content blocks to the plain text a trigger card shows. */
function contentBlocks(content) {
  if (!Array.isArray(content)) return '';
  return content
    .map(block => (block?.type === 'text' ? block.text : ''))
    .filter(text => typeof text === 'string' && text.length > 0)
    .join('\n');
}

const providerSettings = 'llm-pi-ai';
const managedProvider = /^kwork-[a-f0-9-]{36}$/;
const managedCredential = /^KWORK_[A-F0-9]{32}_API_KEY$/;
const openAiProtocols = ['openai-completions', 'openai-responses'];
const geminiProtocol = 'google-generative-ai';
const editableProtocols = new Set([...openAiProtocols, 'anthropic-messages', geminiProtocol]);
let providerMutationTail = Promise.resolve();
let providerSchemaClass;

function providerError(message) {
  return new SessionOperationError('providers/invalid', message);
}

function providerConfiguration(ctx) {
  const config = ctx.get('settings')?.describe().find(form => form.ns === providerSettings);
  if (!config || !ctx.get('credentials')) throw providerError('当前 dsh-acp 不支持编辑模型服务。');
  return { profiles: config.value.providers ?? {}, revision: config.revision, config };
}

function schemaEntryFor(tools) {
  const manifest = createRequire(tools).resolve('@deepseek-ai/schemastery/package.json');
  const entry = JSON.parse(readFileSync(manifest, 'utf8')).exports['.'].import;
  return join(dirname(manifest), entry);
}

async function providerProfileSchema(ctx, config) {
  try {
    if (!providerSchemaClass) {
      const base = createRequire(require.resolve('@deepseek-ai/dsh-base/package.json'));
      providerSchemaClass = (await import(pathToFileURL(schemaEntryFor(base.resolve('@deepseek-ai/dsh-tools'))).href)).default;
    }
    return new providerSchemaClass(config.schema).dict?.providers?.inner?.dict;
  } catch (error) {
    ctx.logger.warn('KWork could not read the provider settings schema: %s', String(error));
    return undefined;
  }
}

function providerProtocols(schema) {
  const protocols = (schema?.api?.list ?? []).map(node => node.value)
    .filter(value => editableProtocols.has(value));
  return protocols.length ? protocols : openAiProtocols;
}

function providerRetrySpec(schema) {
  const number = (node, fallbackMin) => {
    const { min = fallbackMin, max, default: value } = node?.meta ?? {};
    return [min, max, value].every(Number.isFinite) ? { min, max, value } : null;
  };
  const branches = (schema?.list ?? []).map(node => [node.dict?.mode?.value, node.dict]);
  const modes = new Map(branches);
  const normal = modes.get('normal');
  const always = modes.get('always');
  if (!normal || !always) return null;
  const backoff = branch => {
    const fields = branch.backoff?.dict;
    const spec = {
      initialDelayMs: number(fields?.initialDelayMs, Number.MIN_VALUE),
      maxDelayMs: number(fields?.maxDelayMs, Number.MIN_VALUE),
      jitterRatio: number(fields?.jitterRatio),
    };
    return Object.values(spec).every(Boolean) ? spec : null;
  };
  const maxRetries = number(normal.maxRetries);
  const choices = (normal.retryableCodes?.inner?.list ?? [])
    .filter(node => node.type === 'const' && typeof node.value === 'string').map(node => node.value);
  const defaultCodes = normal.retryableCodes?.meta?.default;
  const normalBackoff = backoff(normal);
  const alwaysBackoff = backoff(always);
  if (!maxRetries || !normalBackoff || !alwaysBackoff || !Array.isArray(defaultCodes) || !defaultCodes.length) return null;
  // Earlier Harness versions enforce positive delays in the resolver and
  // expose a plain string array. Keep those routes editable with the official choices.
  return { maxRetries, choices: choices.length ? choices : [
    'EMPTY_RESPONSE', 'MALFORMED_RESPONSE', 'QUOTA', 'RATE_LIMIT', 'SERVER', 'STREAM_CLOSED', 'TIMEOUT', 'TRANSPORT',
  ], defaultCodes, normalBackoff, alwaysBackoff };
}

async function publicProviders(ctx, { profiles, config }) {
  return Promise.all(customProviderIds(ctx, profiles)
    .map(async id => {
      const profile = profiles[id];
      const keyConfigured = typeof profile.apiKeyEnv === 'string' &&
        (await ctx.credentials.describe(profile.apiKeyEnv).catch(() => undefined))?.configured === true;
      return {
        id, name: profile.displayName ?? id, baseUrl: profile.baseURL, protocol: profile.api,
        defaultInput: profile.defaultInput,
        retryPolicy: config.user?.providers?.[id]?.retryPolicy,
        inheritedRetryPolicy: config.base?.providers?.[id]?.retryPolicy,
        editable: providerCanEdit(config, id),
        removable: userOwnsProvider(config, id),
        canClearKey: canClearProviderKey(config, id, profile),
        models: publicProviderModels(profile.models),
        keyConfigured, available: keyConfigured || !profile.apiKeyEnv && profile.api !== geminiProtocol,
      };
    }));
}

function publicProviderModels(models) {
  return (models ?? []).map(({ id, name, contextWindow, maxTokens, input, reasoningEfforts }) => ({
    id, name, contextWindow, maxTokens, inputModalities: input,
    // Keep the ordering of the editable rows across the IPC boundary.
    reasoningEfforts: typeof reasoningEfforts === 'object'
      ? Object.entries(reasoningEfforts ?? {}) : reasoningEfforts,
  }));
}

function isPiAiProvider(entry) {
  return entry.settingsNs === providerSettings && entry.settingsPath?.length === 2 &&
    entry.settingsPath[0] === 'providers' && entry.settingsPath[1] === entry.provider;
}

function installedProviders(ctx) {
  return (ctx.llm?.listConfigurableProviders?.() ?? [])
    .filter(entry => entry.declared === false && isPiAiProvider(entry));
}

function customProviderIds(ctx, profiles) {
  const directory = ctx.llm?.listConfigurableProviders?.();
  const declared = directory?.filter(entry => entry.declared === true && isPiAiProvider(entry))
    .map(entry => entry.provider);
  return (declared ?? Object.keys(profiles).filter(id => managedProvider.test(id)))
    .filter(id => Object.hasOwn(profiles, id) && editableProtocols.has(profiles[id].api) && typeof profiles[id].baseURL === 'string');
}

function providerCanEnterKey(entry, profile) {
  return Boolean(profile?.apiKeyEnv) || entry.nativeAuth?.oauth !== true || entry.nativeAuth.env.length > 0;
}

async function providerSettingsSnapshot(ctx, current) {
  const { profiles, config, revision } = current;
  const providers = await publicProviders(ctx, current);
  const active = new Set(ctx.llm.listProviders().map(entry => entry.id));
  const store = ctx.credentials;
  const records = new Map((await store.listRecords?.().catch(() => []) ?? []).map(entry => [entry.key, entry.kind]));
  const loginFlows = new Map((ctx.get('authorization')?.list() ?? []).map(flow => [flow.key, flow]));
  const infos = new Map();
  const describe = ref => {
    if (!infos.has(ref)) infos.set(ref, store.describe(ref).catch(() => undefined));
    return infos.get(ref);
  };
  const catalog = await Promise.all(installedProviders(ctx).map(async entry => {
    const id = entry.provider;
    const profile = Object.hasOwn(profiles, id) ? profiles[id] : undefined;
    const user = config.user?.providers?.[id];
    const base = config.base?.providers?.[id];
    const ref = profile?.apiKeyEnv;
    const keyInfo = ref ? await describe(ref) : undefined;
    const env = (ref ? [ref] : entry.nativeAuth?.env ?? [])
      .filter(name => /^[A-Z_][A-Z0-9_]*$/i.test(name) && !entry.baseURLEnv?.includes(name));
    const detected = keyInfo?.configured ?? (await Promise.all(env.map(describe))).some(info => info?.configured === true);
    const editable = !profile || providerCanEdit(config, id);
    const loginKey = `${providerSettings}/${id}`;
    const login = loginFlows.get(loginKey);
    return {
      id, name: entry.displayName,
      baseUrl: (editable ? user?.baseURL : profile?.baseURL) ?? '',
      catalog: {
        defaultBaseUrl: base?.baseURL ?? entry.defaultBaseURL ?? '',
        endpointEnv: entry.baseURLEnv ?? [], authEnv: env.filter(name => !/oauth/i.test(name) && !managedCredential.test(name)),
        authDetected: detected,
        oauth: entry.nativeAuth?.oauth === true,
        oauthConfigured: records.get(loginKey) === 'grant',
        signedIn: records.has(loginKey), loginMethods: login?.methods ?? [], loginRunning: login?.inFlight ?? false,
        apiKeyAllowed: providerCanEnterKey(entry, profile),
        diagnostic: entry.error,
        inheritedModels: publicProviderModels(base?.models),
        modelsOverridden: Object.hasOwn(user ?? {}, 'models'),
      },
      keyConfigured: keyInfo?.configured ?? false,
      canClearKey: canClearProviderKey(config, id, profile) && keyInfo?.writable === true,
      available: active.has(id) && (!ref || keyInfo?.configured === true),
      editable, removable: userOwnsProvider(config, id),
      defaultInput: profile?.defaultInput,
      models: publicProviderModels(profile?.models),
      retryPolicy: user?.retryPolicy, inheritedRetryPolicy: base?.retryPolicy,
    };
  }));
  const catalogProviders = [];
  for (const provider of catalog) {
    (Object.hasOwn(profiles, provider.id) ? providers : catalogProviders).push(provider);
  }
  return { providers, catalogProviders, revision };
}

function userOwnsProvider(config, id) {
  return Object.hasOwn(config.user?.providers ?? {}, id) && !Object.hasOwn(config.base?.providers ?? {}, id);
}

function providerCanEdit(config, id) {
  return Object.hasOwn(config.user?.providers ?? {}, id) || Object.hasOwn(config.base?.providers ?? {}, id);
}

function canClearProviderKey(config, id, profile) {
  return config.base?.providers?.[id]?.apiKeyEnv === undefined &&
    (profile?.apiKeyEnv === undefined || profile.apiKeyEnv === config.user?.providers?.[id]?.apiKeyEnv);
}

function referencesCredential(value, ref) {
  if (value === ref) return true;
  return value !== null && typeof value === 'object' && Object.values(value).some(child => referencesCredential(child, ref));
}

async function removeUnusedProviderKey(ctx, id, ref) {
  const derived = `${id.toUpperCase().replace(/[^A-Z0-9]+/g, '_')}_API_KEY`;
  if (typeof ref !== 'string' || !(ref === derived || managedCredential.test(ref))) return;
  if (ctx.settings.describe().some(form => [form.value, form.base, form.user].some(value => referencesCredential(value, ref)))) return;
  if ((await ctx.credentials.describe(ref)).writable) await ctx.credentials.unset(ref);
}

// A configured compatible route is the startup choice only when the built-in
// DeepSeek route cannot authenticate. Explicit session choices stay in place.
async function chooseCompatibleDefault(ctx, agent, profiles, preferred, signal) {
  if (agent.model.current()?.provider !== 'deepseek-official' || agent.session.header.origin === 'subagent') return;
  const settings = ctx.get('settings');
  const credentials = ctx.get('credentials');
  if (!settings || !credentials) return;
  const forms = settings.describe();
  const providers = profiles ?? forms.find(form => form.ns === providerSettings)?.value?.providers ?? {};
  const installed = new Set(installedProviders(ctx).map(entry => entry.provider));
  const registered = new Set(ctx.llm.listProviders().map(entry => entry.id));
  const ids = [...customProviderIds(ctx, providers).filter(id => providers[id].models?.length),
    ...Object.keys(providers).filter(id => installed.has(id) && registered.has(id))];
  const official = forms.find(form => form.ns === 'llm-deepseek')?.value;
  if (registered.has('deepseek-official') &&
      (await credentials.describe(official?.apiKeyEnv ?? 'DEEPSEEK_API_KEY')).configured) return;
  if (registered.has('deepseek-account') && (await ctx.get('deepseekAccount')?.getState?.().catch(() => null))?.status === 'credential-stored') {
    ids.unshift('deepseek-account');
  }
  const preferredIndex = ids.indexOf(preferred);
  if (preferredIndex > 0) ids.unshift(ids.splice(preferredIndex, 1)[0]);
  for (const provider of ids) {
    const profile = providers[provider];
    if (profile?.apiKeyEnv ? !(await credentials.describe(profile.apiKeyEnv)).configured : profile?.api === geminiProtocol) continue;
    let models = profile?.models;
    if (installed.has(provider) || provider === 'deepseek-account') {
      try { models = await ctx.llm.listModels(provider, signal); }
      catch { signal?.throwIfAborted(); continue; }
    }
    for (const { id: model } of models ?? []) {
      try { await ctx.llm.resolveModelInfo(provider, model, signal); }
      catch { signal?.throwIfAborted(); continue; }
      agent.model.select({ provider, model });
      return;
    }
  }
}

function readProviderDraft(params, protocols) {
  const protocol = params.protocol;
  if (!protocols.includes(protocol)) throw providerError('请选择当前 Harness Host 支持的接口协议。');
  return { protocol, baseUrl: readProviderBaseUrl(params.baseUrl, protocol), key: readProviderKey(params.apiKey) };
}

function readProviderBaseUrl(value, protocol) {
  const address = typeof value === 'string' ? value.trim() : '';
  let url;
  try { url = new URL(address); }
  catch { throw providerError('请输入有效的 HTTP 或 HTTPS API 地址。'); }
  if (!/^https?:\/\//i.test(address) || /\s/.test(address) || url.username || url.password || /[?#]/.test(url.href)) {
    throw providerError('API 地址不能包含用户名、密码、查询参数或片段。');
  }
  const baseUrl = url.href.replace(/\/+$/, '');
  const path = url.pathname.replace(/\/+$/, '');
  if (/\/(?:chat\/completions|responses|models)$/.test(path) ||
      protocol === 'anthropic-messages' && /\/v1\/messages$/.test(path) ||
      protocol === geminiProtocol && /\/models\/[^/]+:(?:streamGenerateContent|generateContent)$/.test(path)) {
    throw providerError('请填写 API 基础地址，例如 https://example.com/v1。');
  }
  return baseUrl;
}

function readProviderKey(key) {
  if (key !== undefined && (typeof key !== 'string' || key.length > 8192 ||
      !/^[\x21-\x7e]+$/.test(key) || /^[A-Z0-9_]+=[^=]/.test(key) || /^(['"`]).*\1$/.test(key))) {
    throw providerError('请填写原始 API Key。');
  }
  return key;
}

function sameProviderOrigin(current, baseUrl) {
  if (!current?.baseURL || !URL.canParse(current.baseURL)) return false;
  return new URL(current.baseURL).origin === new URL(baseUrl).origin;
}

async function requireProtocolKey(ctx, draft, current, clearKey) {
  if (draft.protocol !== geminiProtocol || draft.key !== undefined) return;
  if (!clearKey && sameProviderOrigin(current, draft.baseUrl) && current.apiKeyEnv &&
      (await ctx.credentials.describe(current.apiKeyEnv)).configured) return;
  throw providerError('请输入 Gemini API 密钥后继续。');
}

function configuredModels(items, existing, protocol, catalog = false) {
  if (!Array.isArray(items) || (!catalog && !items.length) || items.length > 512) {
    throw providerError(catalog ? '最多可添加 512 个模型。' : '请添加 1 到 512 个模型。');
  }
  const seen = new Set();
  const previousById = new Map(existing?.map(model => [model.id, model]));
  return items.map(item => {
    const id = typeof item?.id === 'string' ? item.id.trim() : '';
    if (!id || id.length > 512 || /[\p{White_Space}\x00-\x20\x7f]/u.test(id) || seen.has(id)) {
      throw providerError('模型 ID 必须唯一，且不能包含空格。');
    }
    seen.add(id);
    // New drafts identify their saved row independently of the editable ID.
    // Older clients identify rows by ID and used null for untouched fields.
    const editable = Object.hasOwn(item, 'sourceId');
    const source = editable ? item.sourceId : id;
    if (source !== null && typeof source !== 'string') throw providerError('无法找到模型草稿对应的配置。');
    const result = { ...previousById.get(source), id };
    if (editable && item.name === null) delete result.name;
    if (typeof item.name === 'string' && item.name.trim()) result.name = item.name.trim().slice(0, 512);
    for (const field of ['contextWindow', 'maxTokens']) {
      if (editable && item[field] === null) delete result[field];
      else if (Number.isSafeInteger(item[field]) && item[field] > 0) result[field] = item[field];
      else if (editable && Object.hasOwn(item, field)) throw providerError('请填写有效的模型令牌数。');
    }
    if (Array.isArray(item.inputModalities)) {
      result.input = item.inputModalities.filter(value => value === 'text' || value === 'image');
    }
    if (Object.hasOwn(item, 'reasoningEfforts')) {
      if (item.reasoningEfforts === null) delete result.reasoningEfforts;
      else result.reasoningEfforts = configuredReasoningEfforts(item.reasoningEfforts);
    }
    if (protocol === 'google-generative-ai' && result.reasoningEfforts &&
        Object.entries(result.reasoningEfforts).some(([level, wire]) =>
          !['minimal', 'low', 'medium', 'high'].includes(level) || wire !== level)) {
      throw providerError('Gemini API 仅接受 minimal、low、medium 和 high 档位，参数值须与档位名相同。');
    }
    return result;
  });
}

function configuredReasoningEfforts(efforts) {
  if (efforts === false) return false;
  const levels = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'];
  const checked = new Map();
  if (!Array.isArray(efforts)) throw providerError('推理等级配置无效。');
  for (const entry of efforts) {
    if (!Array.isArray(entry) || entry.length !== 2) throw providerError('推理档位配置无效。');
    const [level, wire] = entry;
    if (!levels.includes(level) || checked.has(level)) throw providerError('推理档位必须有效且唯一。');
    if (!(typeof wire === 'string' && wire.length > 0) && !(level === 'off' && wire === null)) {
      throw providerError('思考档位的 API 参数值不能为空。');
    }
    checked.set(level, wire);
  }
  if (![...checked.keys()].some(level => level !== 'off')) {
    throw providerError('至少添加一个 Off 以外的档位，或关闭推理等级。');
  }
  return Object.fromEntries(checked);
}

function installedProvider(ctx, id) {
  const entry = installedProviders(ctx).find(entry => entry.provider === id);
  if (!entry) throw providerError('找不到这个内置服务商。');
  return entry;
}

function providerLoginUrl(raw) {
  if (typeof raw !== 'string' || raw.length > 16384 || !URL.canParse(raw)) return undefined;
  const url = new URL(raw);
  if (url.username || url.password) return undefined;
  if (url.protocol === 'https:' || (url.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname))) {
    return url.href;
  }
  return undefined;
}

function providerLoginPrompt(attempt, prompt) {
  const signal = prompt.signal ? AbortSignal.any([prompt.signal, attempt.controller.signal]) : attempt.controller.signal;
  if (signal.aborted) return Promise.reject(signal.reason);
  if (attempt.view.status !== 'running') return Promise.reject(new Error('Login attempt ended'));
  if (attempt.pending) return Promise.reject(new Error('A login prompt is already active'));
  return new Promise((resolve, reject) => {
    const id = randomUUID();
    const view = { id, kind: prompt.kind, message: prompt.message, placeholder: prompt.placeholder, options: prompt.options };
    const clear = () => {
      signal.removeEventListener('abort', withdraw);
      attempt.pending = undefined;
      delete attempt.view.prompt;
      attempt.view.revision++;
    };
    const finish = (error, value) => {
      clear();
      if (error) reject(error);
      else resolve(value);
    };
    const withdraw = () => finish(signal.reason ?? new Error('Login prompt withdrawn'));
    attempt.pending = { id, view, finish };
    attempt.view.prompt = view;
    attempt.view.revision++;
    signal.addEventListener('abort', withdraw, { once: true });
  });
}

function providerLoginNotice(attempt, notice) {
  if (attempt.controller.signal.aborted || attempt.view.status !== 'running') return;
  const view = attempt.view;
  const url = providerLoginUrl(notice.url);
  if (url) {
    view.url = url;
    view.code = notice.code;
    view.instruction = notice.message;
  } else {
    view.message = notice.message;
  }
  view.revision++;
}

function startProviderLogin(ctx, key, method) {
  const authorization = ctx.get('authorization');
  const flow = authorization?.describe(key);
  if (!flow) throw providerError('当前 Host 没有为此服务商提供登录。');
  if (!flow.methods.some(entry => entry.id === method)) throw providerError('请选择可用的登录方式。');
  if (inputEnded || flow.inFlight || providerAuthAttempts.get(key)?.view.status === 'running' || providerAuthLogouts.has(key)) {
    throw providerError('该服务商已有登录操作在进行。');
  }
  const view = { id: randomUUID(), revision: 0, status: 'running' };
  const attempt = { controller: new AbortController(), view, pending: undefined };
  providerAuthAttempts.set(key, attempt);
  const finish = (status, credentialKind) => {
    attempt.pending?.finish(new Error('Login attempt ended'));
    attempt.view = { id: view.id, revision: view.revision + 1, status, credentialKind };
  };
  void authorization.begin({ key, method, signal: attempt.controller.signal, interaction: {
    notify: notice => providerLoginNotice(attempt, notice),
    prompt: prompt => providerLoginPrompt(attempt, prompt),
  } }).then(async outcome => {
    const record = outcome.status === 'authorized'
      ? await ctx.credentials.describeRecord(key).catch(() => undefined) : undefined;
    finish(outcome.status, record?.kind ?? (outcome.status === 'authorized' ? (method === 'oauth' ? 'grant' : 'api-key') : undefined));
  }).catch(() => finish('failed'));
  return view;
}

async function providerAuthorization(ctx, params) {
  const entry = installedProvider(ctx, params.id);
  const key = `${providerSettings}/${entry.provider}`;
  const attempt = providerAuthAttempts.get(key);
  if (params.action === 'start') return startProviderLogin(ctx, key, params.method);
  if (params.action === 'logout') {
    if (attempt?.view.status === 'running' || ctx.get('authorization')?.describe(key)?.inFlight || providerAuthLogouts.has(key)) {
      throw providerError('请先结束正在进行的登录。');
    }
    providerAuthLogouts.add(key);
    try {
      await ctx.credentials.deleteRecord(key);
      return { revision: 0, status: 'signed-out' };
    } catch {
      throw providerError('退出登录失败，请重试。');
    } finally {
      providerAuthLogouts.delete(key);
    }
  }
  if (!['status', 'cancel', 'answer'].includes(params.action)) throw providerError('无法识别这个登录操作。');
  if (!attempt || attempt.view.id !== params.attempt) {
    if (params.action === 'answer') throw providerError('该登录提示已失效。');
    return { id: params.attempt, revision: Number.MAX_SAFE_INTEGER, status: 'expired' };
  }
  if (params.action === 'cancel') {
    if (attempt.view.status === 'running') attempt.controller.abort();
  } else if (params.action === 'answer') {
    const pending = attempt.pending;
    if (!pending || pending.id !== params.prompt) throw providerError('该登录提示已失效。');
    const value = params.value;
    if (typeof value !== 'string' || !value.trim() || value.length > 16384 ||
        (pending.view.kind === 'select' && !pending.view.options?.some(option => option.id === value))) {
      throw providerError('请输入有效的登录信息。');
    }
    pending.finish(undefined, value);
  }
  return attempt.view;
}

function throwProviderWriteError(error) {
  if (error instanceof SessionOperationError) throw error;
  if (error?.code === 'SETTINGS_CONFLICT') throw providerError('模型服务配置已更新。请刷新列表并重新选择服务商。');
  if (String(error?.message).includes('overridden by a home patch or command-line overlay')) {
    throw providerError('该模型服务由启动参数或全局配置覆盖。请在对应的配置中修改。');
  }
  if (String(error?.message ?? error).includes('retryPolicy')) throw providerError('重试策略无效，请检查错误码和退避设置。');
  throw providerError('保存模型服务失败。请检查配置后重试。');
}

async function writeInstalledProvider(ctx, { profiles, revision, config }, params, removing) {
  const entry = installedProvider(ctx, params.id);
  const id = entry.provider;
  const profile = Object.hasOwn(profiles, id) ? profiles[id] : undefined;
  if (params.expectedRevision !== undefined && params.expectedRevision !== revision) {
    throwProviderWriteError({ code: 'SETTINGS_CONFLICT' });
  }
  if (removing && !userOwnsProvider(config, id)) throw providerError('部署配置中的服务商由 Host 管理。');
  if (profile && !removing && !providerCanEdit(config, id)) throw providerError('该服务商由启动参数或全局配置管理。');

  const path = ['providers', id];
  const ops = [];
  const set = (field, value) => ops.push({ op: 'set', path: [...path, field], value });
  const unset = field => ops.push({ op: 'unset', path: [...path, field] });
  let newRef;
  try {
    if (removing) {
      ops.push({ op: 'unset', path });
    } else {
      const user = config.user?.providers?.[id];
      const base = config.base?.providers?.[id];
      const key = readProviderKey(params.apiKey);
      if (key !== undefined && !providerCanEnterKey(entry, profile)) throw providerError('此服务商使用原生登录认证。');
      if (!profile) ops.push({ op: 'set', path, value: {} });

      if (Object.hasOwn(params, 'baseUrl')) {
        if (typeof params.baseUrl !== 'string') throw providerError('请输入有效的 HTTP 或 HTTPS API 地址。');
        const address = params.baseUrl.trim();
        const url = address && readProviderBaseUrl(address, profile?.api);
        if (address !== (user?.baseURL ?? '')) {
          const before = profile?.baseURL ?? entry.defaultBaseURL;
          const after = url || base?.baseURL || entry.defaultBaseURL;
          if (profile?.apiKeyEnv && key === undefined && !params.clearKey && before !== after &&
              (!after || !sameProviderOrigin({ baseURL: before }, after))) {
            throw providerError('更改服务地址后，请重新输入此地址的 API Key，或选择清除原密钥。');
          }
          if (url) set('baseURL', url);
          else unset('baseURL');
        }
      }
      if (Object.hasOwn(params, 'models')) {
        if (params.models === null) unset('models');
        else set('models', configuredModels(params.models, params.modelsFromDefault ? base?.models : profile?.models, undefined, true));
      }
      if (Object.hasOwn(params, 'retryPolicy')) {
        if (params.retryPolicy === null) unset('retryPolicy');
        else set('retryPolicy', params.retryPolicy);
      }
      if (key !== undefined) {
        newRef = 'KWORK_' + randomUUID().replaceAll('-', '').toUpperCase() + '_API_KEY';
        await ctx.credentials.set(newRef, key);
        set('apiKeyEnv', newRef);
      } else if (params.clearKey && profile?.apiKeyEnv) {
        if (!canClearProviderKey(config, id, profile)) throw providerError('此服务商继承了部署密钥。');
        unset('apiKeyEnv');
      }
    }
    if (ops.length) await ctx.settings.mutate(providerSettings, ops, revision);
  } catch (error) {
    if (newRef) await ctx.credentials.unset(newRef).catch(() => {});
    throwProviderWriteError(error);
  }
  return finishProviderWrite(ctx, params, id, removing, profile?.apiKeyEnv);
}

async function finishProviderWrite(ctx, params, id, removing, oldRef) {
  const updated = providerConfiguration(ctx);
  if (oldRef && updated.profiles[id]?.apiKeyEnv !== oldRef) {
    await removeUnusedProviderKey(ctx, id, oldRef).catch(() => {});
  }
  if (!removing && typeof params.sessionId === 'string') {
    const agent = ctx.get('agents')?.get(params.sessionId);
    if (agent?.status === 'idle' && !agent.session.requestHeader()) {
      await chooseCompatibleDefault(ctx, agent, updated.profiles, id).catch(error => {
        ctx.logger.warn('KWork could not select the compatible model: %s', String(error));
      });
    }
  }
  // A model list can change without changing the registered routes.
  try { ctx.emit('llm/adapters-updated'); }
  catch { ctx.logger.warn('KWork could not refresh the model picker'); }
  return { id, ...await providerSettingsSnapshot(ctx, updated) };
}

async function modelProviderOperation(ctx, method, params) {
  if (method === '_dsh/gpui/providers/authorization') {
    return providerAuthorization(ctx, params).catch(error => {
      if (error instanceof SessionOperationError) throw error;
      throw providerError('无法完成此登录操作，请重试。');
    });
  }
  if (method === '_dsh/gpui/providers/list') {
    const current = providerConfiguration(ctx);
    if (!params.includeRetrySettings) return publicProviders(ctx, current);
    const schema = await providerProfileSchema(ctx, current.config);
    return { ...await providerSettingsSnapshot(ctx, current), protocols: providerProtocols(schema), retryPolicySpec: providerRetrySpec(schema?.retryPolicy) };
  }
  if (method === '_dsh/gpui/providers/discover') {
    if (params.catalog === true) {
      const entry = installedProvider(ctx, params.id);
      // The installed catalog answers from its own registry. Sending only
      // the route ID keeps endpoint overrides and credentials out of this read.
      return ctx.llm.discoverModels(providerSettings, { provider: entry.provider }, AbortSignal.timeout(12000))
        .catch(() => { throw providerError('无法读取内置模型目录，请重试或手动添加模型 ID。'); });
    }
    const { profiles, config } = providerConfiguration(ctx);
    const schema = await providerProfileSchema(ctx, config);
    const draft = readProviderDraft(params, providerProtocols(schema));
    const { baseUrl, protocol, key } = draft;
    const current = customProviderIds(ctx, profiles).includes(params.id) ? profiles[params.id] : undefined;
    if (params.id !== undefined && !current) throw providerError('找不到这个自定义服务商。');
    await requireProtocolKey(ctx, draft, current, params.clearKey);
    const provider = params.clearKey || !sameProviderOrigin(current, baseUrl) ? undefined : params.id;
    return ctx.llm.discoverModels(providerSettings, {
      provider, api: protocol, baseURL: baseUrl, apiKey: key,
    }, AbortSignal.timeout(12000)).catch(() => {
      throw providerError('无法同步模型。请检查 API 地址和密钥，或手动添加模型 ID。');
    });
  }

  const operation = providerMutationTail.then(async () => {
    const snapshot = providerConfiguration(ctx);
    const { profiles, revision, config } = snapshot;
    const removing = method === '_dsh/gpui/providers/remove';
    if (params.catalog === true) return writeInstalledProvider(ctx, snapshot, params, removing);
    const editing = removing || params.id !== undefined;
    const id = params.id ?? (removing ? '' : 'kwork-' + randomUUID());
    if (editing && !customProviderIds(ctx, profiles).includes(id)) {
      throw providerError('找不到这个自定义服务商。');
    }
    if (editing && params.expectedRevision !== undefined && params.expectedRevision !== revision) {
      throw providerError('模型服务配置已更新。请刷新列表并重新选择服务商。');
    }
    if (removing && !userOwnsProvider(config, id)) {
      throw providerError('部署配置中的服务商由 Host 管理。');
    }
    if (editing && !removing && !providerCanEdit(config, id)) {
      throw providerError('该服务商由启动参数或全局配置管理。');
    }
    const current = editing ? profiles[id] : undefined;
    const oldRef = current?.apiKeyEnv;
    const path = ['providers', id];
    let newRef;
    try {
      if (removing) {
        await ctx.settings.mutate(providerSettings, [{ op: 'unset', path }], revision);
      } else {
        const schema = await providerProfileSchema(ctx, config);
        const draft = readProviderDraft(params, providerProtocols(schema));
        const { baseUrl, protocol, key } = draft;
        await requireProtocolKey(ctx, draft, current, params.clearKey);
        const name = typeof params.name === 'string' ? params.name.trim() : '';
        if (!name || (name.length > 80 && name !== (current?.displayName ?? id))) {
          throw providerError('请输入不超过 80 个字符的服务商名称。');
        }
        const models = configuredModels(params.models, current?.models, protocol);
        const sameOrigin = sameProviderOrigin(current, baseUrl);
        if (key === undefined && !canClearProviderKey(config, id, current) && (params.clearKey || !sameOrigin)) {
          throw providerError('此服务商继承了部署密钥。更改地址来源时，请输入新的 API Key。');
        }
        let apiKeyEnv = params.clearKey || !sameOrigin ? undefined : oldRef;
        if (key !== undefined) {
          newRef = 'KWORK_' + randomUUID().replaceAll('-', '').toUpperCase() + '_API_KEY';
          await ctx.credentials.set(newRef, key);
          apiKeyEnv = newRef;
        } else if (oldRef && !params.clearKey && !sameOrigin) {
          throw providerError('更改服务地址后，请重新输入此地址的 API Key，或选择清除原密钥。');
        }
        if (!current) {
          const value = { displayName: name, api: protocol, baseURL: baseUrl, models };
          if (params.retryPolicy != null) value.retryPolicy = params.retryPolicy;
          if (apiKeyEnv) value.apiKeyEnv = apiKeyEnv;
          await ctx.settings.mutate(providerSettings, [{ op: 'set', path, value }], revision);
        } else {
          // Edit only fields changed by this form. The effective descriptor
          // can include inherited defaults and redact deployment-owned fields.
          const ops = [];
          const set = (field, value) => ops.push({ op: 'set', path: [...path, field], value });
          const unset = field => ops.push({ op: 'unset', path: [...path, field] });
          if (name !== (current.displayName ?? id)) set('displayName', name);
          if (protocol !== current.api) set('api', protocol);
          if (params.baseUrl.trim() !== current.baseURL) set('baseURL', baseUrl);
          if (JSON.stringify(models) !== JSON.stringify(current.models)) set('models', models);
          if (Object.hasOwn(params, 'retryPolicy')) {
            if (params.retryPolicy === null) unset('retryPolicy');
            else set('retryPolicy', params.retryPolicy);
          }
          if (apiKeyEnv !== oldRef) {
            if (apiKeyEnv === undefined) unset('apiKeyEnv');
            else set('apiKeyEnv', apiKeyEnv);
          }
          if (ops.length) await ctx.settings.mutate(providerSettings, ops, revision);
        }
      }
    } catch (error) {
      if (newRef) await ctx.credentials.unset(newRef).catch(() => {});
      throwProviderWriteError(error);
    }
    return finishProviderWrite(ctx, params, id, removing, oldRef);
  });
  providerMutationTail = operation.then(() => undefined, () => undefined);
  return operation;
}

async function deepSeekCallbackOrigin(ctx) {
  const mounted = ctx.get('webServer');
  if (!mounted) {
    accountCallback ??= (async () => {
      const local = join(service, 'packages/host/webserver', profileEntry.endsWith('.ts') ? 'src/index.ts' : 'lib/index.js');
      const entry = existsSync(local) ? local : require.resolve('@deepseek-ai/dsh-host-webserver');
      const { default: WebServer } = await import(pathToFileURL(entry).href);
      const fiber = ctx.plugin(WebServer, { host: '127.0.0.1', port: 0 });
      accountCallbackFiber = fiber;
      await fiber;
    })().catch(async error => {
      await accountCallbackFiber?.dispose().catch(() => {});
      accountCallbackFiber = undefined;
      accountCallback = undefined;
      throw error;
    });
    await accountCallback;
  } else if (accountCallback) {
    await accountCallback;
  }
  const origin = ctx.get('webServer')?.getOrigin();
  if (!/^http:\/\/(?:127\.0\.0\.1|localhost|\[::1\]):[1-9]\d*$/.test(origin)) {
    throw new SessionOperationError('account/callback', '未能启动 DeepSeek 登录所需的本机回调。');
  }
  return origin;
}

function accountWireState(ctx, state) {
  const attempt = state.attempt ? { ...state.attempt, authorizeUrl: providerLoginUrl(state.attempt.authorizeUrl) } : null;
  return {
    available: true,
    platformEmbedded: typeof ctx.get('deepseekAccount')?.getPlatformSession === 'function',
    notifications: state.status === 'credential-stored' ? notificationCapability(ctx) : null,
    modelReady: state.status === 'credential-stored' && ctx.llm?.listProviders().some(route => route.id === 'deepseek-account'),
    state: { status: state.status, attempt, links: {
      usageUrl: providerLoginUrl(state.links?.usageUrl), topUpUrl: providerLoginUrl(state.links?.topUpUrl),
    } },
  };
}

function notificationCapability(ctx) {
  const account = ctx.get('deepseekAccount');
  if (!account) return null;
  const modern = ['getNotifications', 'getNotificationContent', 'reportNotificationEvent']
    .every(name => typeof account[name] === 'function');
  const legacy = ['getUnnotifiedBonuses', 'ackBonusNotified'].every(name => typeof account[name] === 'function');
  if (!modern && !legacy) return null;
  const configuration = ctx.get('configEditor')?.configuration?.()
    .find(({ entry }) => entry.options.id === 'ui-account-notifications');
  const configured = configuration?.entry.fiber?.effectiveConfig?.notificationPollIntervalMs;
  const pollIntervalMs = Number.isSafeInteger(configured) && configured > 0 && configured <= 2_147_483_647
    ? configured : 600_000;
  return { scope: notificationScope, mode: modern ? 'modern' : 'bonus', pollIntervalMs };
}

function notificationForClient(notice) {
  if (!notice || typeof notice.id !== 'string' || typeof notice.title !== 'string'
    || typeof notice.body !== 'string' || !Array.isArray(notice.actions)) return null;
  const actions = notice.actions.flatMap(action => {
    if (!action || typeof action.label !== 'string' || !['primary', 'secondary'].includes(action.style)) return [];
    const target = action.target;
    if (target?.type !== 'dismiss' && (target?.type !== 'app_route' || target.name !== 'account')) return [];
    return [{ label: action.label, style: action.style, target: target.type === 'dismiss' ? 'dismiss' : 'account' }];
  });
  return {
    id: notice.id, title: notice.title, body: notice.body, actions,
    displayUntil: typeof notice.displayUntil === 'string' && notice.displayUntil ? notice.displayUntil : null,
  };
}

async function accountNotifications(ctx, params) {
  const account = ctx.get('deepseekAccount');
  const capability = notificationCapability(ctx);
  const scope = params.scope;
  if (!capability || scope !== capability.scope) return null;
  const version = typeof params.version === 'string' && /^[\dA-Za-z.+-]{1,64}$/.test(params.version) ? params.version : '0.1.0';
  const locale = typeof params.locale === 'string' && /^[a-z]{2,3}(?:[-_][a-zA-Z0-9]{2,8}){0,3}$/i.test(params.locale)
    ? params.locale : 'zh-CN';
  const offset = params.timezoneOffsetSeconds;
  const client = { version, locale, timezoneOffsetSeconds: Number.isSafeInteger(offset) && Math.abs(offset) <= 86_400
    ? offset : -new Date().getTimezoneOffset() * 60 };
  try {
    if (params.action === 'list') {
      if (capability.mode === 'modern') {
        const batch = await account.getNotifications(client);
        if (notificationScope !== scope || batch == null) return null;
        return { nextPollAfterSeconds: batch.nextPollAfterSeconds, notifications: batch.notifications.map(notificationForClient).filter(Boolean) };
      }
      const batch = await account.getUnnotifiedBonuses(client);
      if (notificationScope !== scope || batch == null) return null;
      legacyNotificationAccount = batch.accountId;
      const chinese = /^zh(?:[-_]|$)/i.test(locale);
      return { nextPollAfterSeconds: capability.pollIntervalMs / 1000, notifications: batch.bonuses.map(bonus => ({
        id: bonus.orderId, title: bonus.title || (chinese ? '赠金到账' : 'Bonus credited'), body: bonus.message, displayUntil: bonus.expiresAt || null,
        actions: [{ label: chinese ? '查看余额' : 'View balance', style: 'primary', target: 'account' }],
      })) };
    }
    if (typeof params.id !== 'string') return null;
    if (params.action === 'content' && capability.mode === 'modern') {
      const notice = await account.getNotificationContent(params.id, client);
      return notificationScope === scope && notice?.id === params.id ? notificationForClient(notice) : null;
    }
    if (params.action === 'report' && notificationEvents.has(params.event)) {
      if (capability.mode === 'modern') {
        return await account.reportNotificationEvent(params.id, params.event, client);
      }
      if (params.event === 'displayed' && legacyNotificationAccount != null) {
        return await account.ackBonusNotified(legacyNotificationAccount, params.id, client);
      }
      return false;
    }
    return null;
  } catch {
    if (notificationScope !== scope) return null;
    throw new SessionOperationError('account/notifications', '账号通知请求失败。');
  }
}

function runningAccountTasks(ctx, account) {
  return account.hasRunningAccountTasks?.() ?? (ctx.get('agents')?.list().some(agent =>
    agent.status === 'running' && agent.session.requestContext()?.provider === 'deepseek-account') ?? false);
}

async function deepSeekAccountOperation(ctx, params) {
  const account = ctx.get('deepseekAccount');
  if (typeof account?.getState !== 'function' || typeof account.startSignIn !== 'function') return { available: false };
  const version = typeof params.version === 'string' && /^[\dA-Za-z.+-]{1,64}$/.test(params.version) ? params.version : '0.1.0';
  const client = { version, locale: 'zh-CN', timezoneOffsetSeconds: -new Date().getTimezoneOffset() * 60 };
  try {
    let state;
    if (params.action === 'start') {
      const origin = await deepSeekCallbackOrigin(ctx);
      if (inputEnded) throw new SessionOperationError('account/closed', '登录连接已关闭。');
      accountLoginService = account;
      state = await account.startSignIn(client, origin, 'desktop');
    } else if (params.action === 'cancel') {
      state = await account.cancelSignIn(typeof params.attempt === 'string' ? params.attempt : '');
    } else if (params.action === 'signout') {
      if (params.approveRunning !== true && await runningAccountTasks(ctx, account)) {
        return { ...accountWireState(ctx, await account.getState()), runningTasks: true, needsConfirmation: true };
      }
      state = await account.signOut(client);
    } else if (params.action === 'check-signout') {
      return { ...accountWireState(ctx, await account.getState()), runningTasks: await runningAccountTasks(ctx, account) };
    } else if (params.action === 'details') {
      const [profile, balance] = await Promise.allSettled([account.getProfile(client), account.getBalance(client)]);
      return { ...accountWireState(ctx, await account.getState()), details: {
        profile: profile.status === 'fulfilled' ? profile.value : { status: 'failed' },
        balance: balance.status === 'fulfilled' ? balance.value : { status: 'failed' },
      } };
    } else if (params.action === 'status') {
      state = await account.getState();
      if (state.status === 'credential-stored' && typeof params.sessionId === 'string') {
        const agent = ctx.get('agents')?.get(params.sessionId);
        if (agent?.status === 'idle' && !agent.session.requestHeader()) {
          await chooseCompatibleDefault(ctx, agent, undefined, 'deepseek-account').catch(() => {});
        }
      }
    } else {
      throw new SessionOperationError('account/action', '无法识别此账号操作。');
    }
    return accountWireState(ctx, state);
  } catch (error) {
    if (error instanceof SessionOperationError) throw error;
    throw new SessionOperationError('account/unavailable', 'DeepSeek 账号操作失败，请重试。');
  }
}

function observeDeepSeekAccount(ctx) {
  const publish = async reason => {
    const state = await ctx.get('deepseekAccount')?.getState?.().catch(() => null);
    const ready = state ? accountWireState(ctx, state).modelReady : false;
    process.stdout.write(JSON.stringify({ jsonrpc: '2.0', method: '_dsh/gpui/deepseek-account-changed', params: { ready, reason } }) + '\n');
  };
  ctx.on('credentials/record-updated', key => {
    if (key !== 'deepseek-account-platform/default') return;
    notificationScope = randomUUID();
    legacyNotificationAccount = undefined;
    void readPlatformSession();
    void publish('changed').catch(() => {});
  }, { global: true });
  for (const reason of ['session-expired', 'model-sign-in-required']) {
    ctx.on(`deepseek-account/${reason}`, () => { void publish(reason).catch(() => {}); }, { global: true });
  }
}

let platformSessionSource;
let platformSession = null;
let platformSessionRead = Promise.resolve(null);

function updatePlatformSession(next) {
  const previous = platformSession;
  platformSession = next;
  if (previous && (previous.token !== next?.token || previous.origin !== next?.origin
    || previous.embeddedPageDist !== next?.embeddedPageDist
    || JSON.stringify(previous.requestHeaders) !== JSON.stringify(next?.requestHeaders)
    || (previous.userId && previous.userId !== next?.userId))) {
    // This private event carries no token, account ID, or deployment headers.
    process.stdout.write('{"jsonrpc":"2.0","method":"_dsh/gpui/account-platform-session-revoked"}\n');
  }
}

function readPlatformSession() {
  const source = platformSessionSource;
  if (!source) return Promise.resolve(null);
  // A concurrent open and identity update must publish in the same order.
  const read = platformSessionRead.then(async () => {
    if (platformSessionSource !== source) return null;
    const next = await source.account.getPlatformSession().catch(() => null) ?? null;
    if (platformSessionSource !== source) return null;
    updatePlatformSession(next);
    return next;
  });
  platformSessionRead = read.catch(() => null);
  return platformSessionRead;
}

function observePlatformSession(ctx) {
  ctx.inject(['deepseekAccount'], accountCtx => {
    const account = accountCtx.get('deepseekAccount');
    if (typeof account?.getPlatformSession !== 'function' || typeof account.watch !== 'function') return;
    accountCtx.effect(() => {
      const lifetime = new AbortController();
      const source = { account, lifetime };
      platformSessionSource = source;
      const clear = () => {
        if (platformSessionSource !== source) return;
        platformSessionSource = undefined;
        platformSessionRead = Promise.resolve(null);
        updatePlatformSession(null);
      };
      const updates = (async () => {
        try {
          for await (const _state of account.watch(lifetime.signal, true)) {
            if (lifetime.signal.aborted) break;
            await readPlatformSession();
          }
        } catch {
          if (!lifetime.signal.aborted) console.error('KWork: 平台账号状态订阅结束。');
        } finally {
          clear();
        }
      })();
      return async () => {
        lifetime.abort();
        clear();
        await updates;
      };
    });
  });
}

// The stock ACP configuration reply discards individual provider failures.
// Read the same registry and validate the same routes as Harness modelCatalog.
async function readModelCatalog(ctx) {
  const llm = ctx.get('llm');
  if (!llm) return { available: false };
  const account = await ctx.get('deepseekAccount')?.getState?.().catch(() => null);
  const providers = llm.listProviders().filter(provider => provider.id !== 'deepseek-account' || account?.status === 'credential-stored');
  const catalog = await Promise.all(providers.map(async ({ id, name }) => {
    try {
      const models = await llm.listModels(id);
      const options = await Promise.all(models.map(async model => {
        await llm.resolveModelInfo(id, model.id);
        return {
          value: JSON.stringify([id, model.id]),
          name: model.name,
          ...(model.description === undefined ? {} : { description: model.description }),
        };
      }));
      return { group: { group: id, name, options } };
    } catch (error) {
      return { failure: { id, name, message: error instanceof Error ? error.message : String(error) } };
    }
  }));
  return {
    available: true,
    groups: catalog.flatMap(entry => entry.group?.options.length ? [entry.group] : []),
    failures: catalog.flatMap(entry => entry.failure ? [entry.failure] : []),
  };
}

function observeModelProviders(ctx) {
  let scheduled = false;
  const notifyCatalogChanged = () => {
    if (scheduled) return;
    scheduled = true;
    setImmediate(() => {
      scheduled = false;
      process.stdout.write('{"jsonrpc":"2.0","method":"_dsh/gpui/model-catalog-changed"}\n');
    });
  };
  for (const event of [
    'llm/adapters-updated', 'llm/models-updated', 'settings/document-updated',
    'credentials/record-updated', 'credentials/reference-updated', 'authorization/settled',
  ]) ctx.on(event, notifyCatalogChanged, { global: true });
  ctx.on('llm-pi-ai/request-headers', (request, headers, next) => {
    if (managedProvider.test(request.provider) && !headers.has('authorization')) {
      const profile = providerConfiguration(ctx).profiles[request.provider];
      // The OpenAI SDK requires an Authorization value even for a local
      // gateway without a key. This placeholder carries no credential.
      if (profile && openAiProtocols.includes(profile.api) && !profile.apiKeyEnv) headers.set('authorization', 'Bearer kwork-no-key');
    }
    return next();
  }, { global: true });
  ctx.on('agent/created', async ({ agent, source, signal }) => {
    if (source === 'startup') {
      await chooseCompatibleDefault(ctx, agent, undefined, undefined, signal).catch(error => {
        signal?.throwIfAborted();
        ctx.logger.warn('KWork could not resolve a compatible startup model: %s', String(error));
      });
    }
    const selection = agent.model.current();
    if (!selection || !managedProvider.test(selection.provider)) return;
    try { await ctx.llm.resolveModelInfo(selection.provider, selection.model); }
    catch {
      const fallback = ctx.get('agentDefaultModel')?.currentSelection();
      if (fallback && !managedProvider.test(fallback.provider)) agent.model.select(fallback);
    }
  }, { global: true });
}

function questionError(code, message) {
  return Object.assign(new Error(message), { name: 'UserQuestionError', code });
}

function startQuestions(ctx) {
  return startingQuestions ??= (async () => {
    observeQuestions(ctx);
    await mountQuestionTool(ctx).catch(error => {
      ctx.logger.warn('GPUI could not mount ask_user_question: %s', String(error));
    });
  })();
}

async function mountQuestionTool(ctx) {
  if (!ctx.get('userQuestions') || ctx.get('tools')?.get('ask_user_question') || typeof bundledAskUser !== 'string') return;
  const base = createRequire(require.resolve('@deepseek-ai/dsh-base/package.json'));
  const tools = base.resolve('@deepseek-ai/dsh-tools');
  const dependencies = {
    '@deepseek-ai/dsh-tools': tools,
    '@deepseek-ai/dsh-user-questions': base.resolve('@deepseek-ai/dsh-user-questions'),
    '@deepseek-ai/schemastery': schemaEntryFor(tools),
  };
  // The vendor module is the installed Harness build. Bind it to this
  // profile's service instances instead of importing a second Cordis runtime.
  const source = bundledAskUser.replace(/from "(@deepseek-ai\/[^"]+)"/g,
    (_match, name) => 'from ' + JSON.stringify(pathToFileURL(dependencies[name]).href));
  const plugin = await import('data:text/javascript;base64,' + Buffer.from(source).toString('base64'));
  await ctx.plugin(plugin, {});
}

function questionProjection(ctx, agent) {
  if (!agent || agent.session.header.origin === 'subagent') return { rows: [], settled: [], queued: [] };
  const { userQuestions } = ctx.get('sessionProjections')?.snapshot(agent.session, ['userQuestions']).values ?? {};
  const replies = new Map([...agent.inbox.nextTurn, ...agent.inbox.nextStep]
    .filter(message => message.source.kind === 'user-question-reply').map(message => [message.source.callId, message]));
  const queued = [];
  for (const [callId, message] of replies) {
    try {
      const payload = JSON.parse(message.content.find(block => block.type === 'text')?.text ?? '');
      if (payload?.kind === 'answer_to_pending_question' && payload.callId === callId && Array.isArray(payload.answers)) {
        queued.push({ callId, answers: payload.answers });
      }
    } catch { /* Leave malformed replies out of the review surface. */ }
  }
  return {
    rows: (userQuestions?.active ?? []).filter(row => row.state === 'continued' && !replies.has(row.callId)),
    settled: userQuestions?.settled ?? [],
    queued,
  };
}

const previousContinuedQuestions = new WeakMap();
function publishContinuedQuestions(ctx, agent, force = false) {
  const view = questionProjection(ctx, agent);
  if (agent) {
    const fingerprint = JSON.stringify(view);
    const previous = previousContinuedQuestions.get(agent);
    if (force || (previous !== undefined && fingerprint !== previous)
        || (previous === undefined && (view.rows.length || view.settled.length || view.queued.length))) {
      previousContinuedQuestions.set(agent, fingerprint);
      notify(agent.id, { sessionUpdate: 'gpui_questions', phase: 'continued', ...view });
    }
  }
  return view.rows;
}

function observeQuestions(ctx) {
  const callsBySession = new WeakMap();
  ctx.on('user-questions/request', async ({ agent, questions, signal, wait }, next) => {
    if (!agent || questions.some(question => question.intent !== undefined)) return next();
    const lifetime = new AbortController();
    const stream = wait?.timed && wait.callId
      ? ctx.get('userQuestions').attachWait(agent, wait.callId, lifetime.signal) : undefined;
    const frame = await stream?.next();
    if (frame?.done || signal?.aborted) {
      lifetime.abort();
      await stream?.return();
      return next();
    }
    const requestId = wait?.callId ?? randomUUID();
    const key = JSON.stringify([agent.id, requestId]);
    const result = Promise.withResolvers();
    const channel = { result, questions, timed: frame !== undefined, timedOut: false };
    const abort = () => {
      channel.timedOut = signal?.reason?.code === 'ASK_TIMED_OUT';
      result.reject(questionError('ASK_ABORTED', 'The question request was aborted'));
    };
    signal?.addEventListener('abort', abort, { once: true });
    questionRequests.set(key, channel);
    const update = { sessionUpdate: 'gpui_questions', requestId };
    notify(agent.id, { ...update, phase: 'open', questions, callId: wait?.callId,
      remainingMs: frame?.value.remainingMs });
    try {
      if (signal?.aborted) abort();
      return await result.promise;
    } finally {
      signal?.removeEventListener('abort', abort);
      lifetime.abort();
      await stream?.return();
      questionRequests.delete(key);
      notify(agent.id, { ...update, phase: 'closed', willContinue: channel.timedOut });
    }
  }, { global: true });

  ctx.on('session/event', (session, event) => {
    const { type, data, surfaceOp } = event;
    if (type === 'tool/call' && data.name === 'ask_user_question') {
      let calls = callsBySession.get(session);
      if (!calls) callsBySession.set(session, calls = new Map());
      calls.set(data.callId, data.arguments);
    } else if (type === 'tool/result' && surfaceOp === 'append') {
      const calls = callsBySession.get(session);
      const id = data.message.source?.callId ?? data.message.toolCallId;
      if (calls?.has(id)) {
        const questionArguments = calls.get(id);
        calls.delete(id);
        notify(session.id, { sessionUpdate: 'gpui_question_record', event: { ...event, questionArguments } });
      }
    } else if (type === 'user/message' && surfaceOp === 'append' && data.source?.kind === 'user-question-reply') {
      notify(session.id, { sessionUpdate: 'gpui_question_record', event });
    }
    if (event.type === 'tool/result' || event.type === 'tool/ptc-dispatch' || event.type === 'turn/end' ||
        (event.type === 'user/message' && event.data.source?.kind === 'user-question-reply')) {
      publishContinuedQuestions(ctx, ctx.get('agents').get(session.id));
    }
  }, { global: true });
  for (const name of ['agent/inbox/inserted', 'agent/inbox/discarded']) {
    ctx.on(name, ({ agent, message }) => {
      if (message.source.kind === 'user-question-reply') publishContinuedQuestions(ctx, agent);
    }, { global: true });
  }
}

function validateAnswers(questions, answers) {
  const ids = new Set();
  if (!Array.isArray(answers) || answers.length !== questions.length || answers.some(answer => {
    if (!answer || ids.has(answer.id) || !Array.isArray(answer.selected)) return true;
    ids.add(answer.id);
    const question = questions.find(question => question.id === answer.id);
    if (!question || new Set(answer.selected).size !== answer.selected.length ||
        answer.selected.some(label => !question.options?.some(option => option.label === label))) return true;
    if (answer.custom !== undefined && (typeof answer.custom !== 'string' || !answer.custom.trim())) return true;
    return !question.multiSelect && answer.selected.length + Number(answer.custom !== undefined) > 1;
  })) throw new SessionOperationError('question/invalid-answer', '答案与待回答的问题不匹配。');
  return { answers };
}

function respondQuestion(ctx, { sessionId, requestId, decision, answers }) {
  const key = JSON.stringify([sessionId, requestId]);
  const channel = questionRequests.get(key);
  if (channel) {
    if (decision === 'answer') {
      const batch = validateAnswers(channel.questions, answers);
      questionRequests.delete(key);
      channel.result.resolve(batch);
    } else if (decision === 'cancel' && !channel.timed) {
      questionRequests.delete(key);
      channel.result.reject(questionError('ASK_CANCELLED', 'The user dismissed the questions'));
    } else if (decision === 'timeout' && channel.timed) {
      questionRequests.delete(key);
      channel.timedOut = true;
      channel.result.reject(questionError('ASK_TIMED_OUT', 'The question wait expired'));
    } else {
      throw new SessionOperationError('question/invalid-decision');
    }
    return {};
  }
  if (decision === 'answer') {
    const agent = ctx.get('agents').get(sessionId);
    const row = questionProjection(ctx, agent).rows.find(row => row.callId === requestId);
    if (row) {
      const batch = validateAnswers(row.questions, answers);
      try {
        if (ctx.get('userQuestions').answer(agent, requestId, batch)) return {};
      } catch (error) {
        if (error?.name === 'UserQuestionError' && error.code === 'REPLY_QUEUED') {
          throw new SessionOperationError('question/reply-queued', '这些问题已提交。');
        }
        throw error;
      }
    }
  }
  throw new SessionOperationError('question/closed', '这些问题已结束或已提交。');
}

function observePlanReviews(ctx) {
  ctx.on('user-questions/request', async ({ agent, questions, signal, wait }, next) => {
    const [question] = questions;
    if (!agent || wait || question?.intent?.kind !== 'plan-review') return next();
    const requestId = randomUUID();
    const sessionId = agent.id;
    const review = Promise.withResolvers();
    const abort = () => review.reject(questionError('ASK_ABORTED', 'Plan review was aborted'));
    signal?.addEventListener('abort', abort, { once: true });
    const { approve, callId } = question.intent;
    const channel = { sessionId, review, questionId: question.id, approve };
    const update = { sessionUpdate: 'gpui_plan_review', requestId };
    planReviews.set(requestId, channel);
    notify(sessionId, {
      ...update, phase: 'open', callId, question: question.question, markdown: question.detail,
      approvalDescription: question.options.find(option => option.label === approve)?.description,
    });
    try {
      if (signal?.aborted) abort();
      return await review.promise;
    } finally {
      signal?.removeEventListener('abort', abort);
      planReviews.delete(requestId);
      notify(sessionId, { ...update, phase: 'closed' });
    }
  }, { global: true });
}

function respondPlanReview({ sessionId, requestId, decision }) {
  const channel = planReviews.get(requestId);
  if (channel?.sessionId !== sessionId) {
    throw new SessionOperationError('plan/review-closed', '该计划已结束审阅。');
  }
  if (decision !== 'approve' && decision !== 'discuss') {
    throw new SessionOperationError('plan/invalid-decision');
  }
  planReviews.delete(requestId);
  const { review, questionId, approve } = channel;
  if (decision === 'approve') {
    review.resolve({ answers: [{ id: questionId, selected: [approve] }] });
  } else {
    review.reject(questionError('ASK_CANCELLED', 'The user dismissed the plan review'));
  }
  return {};
}

function planMode(ctx, { sessionId, active }) {
  const agent = ctx.get('agents').get(sessionId);
  const mode = ctx.get('planMode');
  if (!agent || !mode) throw new SessionOperationError('plan/unavailable', '当前会话不支持计划模式。');
  if (typeof active === 'boolean') mode.set(agent, active);
  const state = mode.get(agent);
  return { active: state.pending ?? state.active };
}

function sessionCommands(ctx, { sessionId }) {
  const agent = ctx.get('agents')?.get(sessionId);
  const commands = ctx.get('commands');
  if (!agent || !commands) return [];
  // The session-scoped catalog is the same source Harness uses for its "/" menu.
  return commands.list(agent).map(({ name, description, input }) => ({
    name,
    description,
    ...input?.hint === undefined ? {} : { inputHint: input.hint },
    ...input?.attachments === true ? { attachments: true } : {},
  }));
}

const commandFileReceipts = new WeakMap();

function commandReceiptStore(ctx, commands) {
  // Cordis can return a fresh service proxy on each lookup.
  let receipts = commandFileReceipts.get(ctx);
  if (!receipts) {
    receipts = new Map();
    commands.registerFileReceiptResolver((agent, id) => {
      const receipt = receipts.get(id);
      return receipt?.agent === agent ? receipt.file : undefined;
    });
    commandFileReceipts.set(ctx, receipts);
  }
  return receipts;
}

async function executeSessionCommand(ctx, { sessionId, line, attachments }) {
  const name = typeof line === 'string' ? /^\/([a-z][a-z0-9_-]*)(?=$|[\t\n\r ])/.exec(line)?.[1] : undefined;
  if (!name) {
    throw new SessionOperationError('command/invalid', '无效的斜杠指令。');
  }
  const agent = ctx.get('agents')?.get(sessionId);
  const commands = ctx.get('commands');
  if (!agent || !commands) {
    throw new SessionOperationError('command/unavailable', '当前会话不支持该指令。');
  }
  const submitted = Array.isArray(attachments) ? attachments.slice() : [];
  const files = submitted.flatMap((block, index) => block?.type === 'file' ? [{ block, index }] : []);
  const signal = new AbortController().signal;
  const fileUploads = ctx.get('fileUploads');
  const batchId = files.length > 0 && fileUploads ? randomUUID() : undefined;
  const minted = [];
  let localReceipts;
  try {
    if (files.length > 0) {
      const descriptor = commands.list(agent).find(command => command.name === name);
      if (!descriptor) throw new SessionOperationError('command/unknown', `未知指令 /${name}。`);
      if (descriptor.input?.attachments !== true) {
        throw new SessionOperationError('command/rejected', `/${name} 不支持附件。`);
      }
      const reject = message => new SessionOperationError('command/attachment-invalid', message);
      const sources = await Promise.all(files.map(({ block }) => stagedPromptFile(block, reject)));
      const store = ctx.get('attachments');
      if (!fileUploads) {
        if (!store || typeof commands.registerFileReceiptResolver !== 'function') {
          throw reject('当前服务不支持指令文件附件。');
        }
        localReceipts = commandReceiptStore(ctx, commands);
      }
      for (const [position, source] of sources.entries()) {
        const receiptId = await readStagedFile(source, async (data, fileName) => {
          if (fileUploads) {
            const { receiptId, file } = await fileUploads.uploadStream({ sessionId, data, name: fileName, signal });
            fileUploads.bindPrompt(agent, [receiptId], batchId).commit();
            if (file.bytes !== source.size) throw reject('附件长度与上传时不符。');
            return receiptId;
          }
          const file = await store.saveFileStream({ data, name: fileName, signal });
          if (file.bytes !== source.size) throw reject('附件长度与上传时不符。');
          const id = randomUUID();
          localReceipts.set(id, { agent, file });
          minted.push(id);
          return id;
        });
        submitted[files[position].index] = { type: 'file', receiptId };
      }
      if (ctx.get('agents')?.get(sessionId) !== agent) {
        throw reject('上传期间会话已关闭，草稿已保留。');
      }
    }
    const execution = await commands.execute(agent, line, submitted, signal);
    if (!execution) throw new SessionOperationError('command/unknown', `未知指令 /${name}。`);
    if (execution.result.kind === 'error') {
      throw new SessionOperationError('command/rejected', execution.result.text || '指令执行失败。');
    }
    const planning = name === 'plan' ? ctx.get('planMode')?.get(agent) : undefined;
    return {
      kind: 'success',
      text: execution.result.text ?? '',
      commandId: execution.commandId,
      ...(planning === undefined ? {} : { planActive: planning.pending ?? planning.active }),
    };
  } finally {
    if (batchId !== undefined) fileUploads.retirePrompt(agent, batchId);
    for (const id of minted) localReceipts.delete(id);
  }
}

async function permissionMode(ctx, { sessionId, preset, acknowledged }) {
  const agent = ctx.get('agents').get(sessionId);
  const permissions = ctx.get('permissionPresets');
  if (!agent || agent.session.header.origin === 'subagent' || !permissions) {
    throw new SessionOperationError('permission/unavailable', '当前会话不支持切换权限。');
  }
  const { options } = permissions.catalog();
  if (preset != null) {
    if (!options.some(option => option.value === preset)) {
      throw new SessionOperationError('permission/invalid-preset', '该权限模式已不可用。');
    }
    if (preset !== permissions.current(agent.session)) {
      if ((preset === 'danger-full-access' || preset === 'auto') && acknowledged !== true) {
        throw new SessionOperationError('permission/confirmation-required', '请先确认权限变更。');
      }
      const execution = await ctx.get('commands')?.execute(
        agent, `/permission ${preset}`, [], new AbortController().signal,
      );
      if (execution?.result.kind !== 'success') {
        throw new SessionOperationError('permission/switch-failed',
          execution?.result.text ?? '当前会话无法切换权限。');
      }
    }
  }
  const currentValue = permissions.current(agent.session);
  return { currentValue, options };
}

function submittedPlanEvents(session, end) {
  const plans = [];
  for (let seq = end - 1; seq >= 0; --seq) {
    const event = session.eventAt(seq);
    if (!event || event.type === 'turn/start' || event.type === 'turn/end') break;
    if (event.data?.name === 'exit_plan_mode' &&
        (event.type === 'tool/call' || event.type === 'tool/ptc-dispatch-start' ||
         event.type === 'tool/ptc-dispatch')) {
      plans.push(event);
    }
  }
  return plans.reverse();
}

// --- Durable mid-turn submissions -------------------------------------------
// Everything below reuses the Host's own Inbox: the message is admitted with
// the same content vocabulary ACP prompts use (`admitAcpPrompt`), then either
// appended to the next turn (`followup`) or steered into the running one.

/** Accepted inline-image media types, identical to the ACP prompt admission. */
const QUEUE_IMAGE_TYPES = new Set(['image/png', 'image/jpeg', 'image/webp', 'image/gif']);
/** Canonical base64: whole quantum groups, one trailing padded group at most. */
const CANONICAL_BASE64 = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;

let llmModule;
/** The Host's own LLM module, so message identity and freezing stay one implementation. */
async function llmApi() {
  return llmModule ??= await import(pathToFileURL(
    createRequire(require.resolve('@deepseek-ai/dsh-base/package.json'))
      .resolve('@deepseek-ai/dsh-llm')).href);
}

function queueRejected(message) {
  return new SessionOperationError('session/queue-invalid', message);
}

async function stagedPromptFile(block, reject) {
  const directory = process.env.KWORK_PROMPT_FILES;
  if (!directory || typeof block.stagingId !== 'string'
    || !/^file-[A-Za-z0-9]{16}$/.test(block.stagingId)
    || !Number.isSafeInteger(block.size) || block.size < 0 || typeof block.name !== 'string') {
    throw reject('附件暂存无效，请重新发送。');
  }
  const path = join(directory, block.stagingId);
  const info = await lstat(path).catch(() => undefined);
  if (!info?.isFile() || info.size !== block.size) {
    throw reject('附件上传未完成，请重新发送。');
  }
  return { path, name: block.name, size: block.size };
}

async function readStagedFile({ path, name }, read) {
  const data = createReadStream(path);
  try {
    return await read(data, name);
  } finally {
    data.destroy();
  }
}

/**
 * Admit one composer payload into ordered core content.
 * Validate every block first, persist the attachments, then rebuild their order.
 */
async function queuePromptContent(ctx, agent, blocks) {
  if (!Array.isArray(blocks) || blocks.length === 0) throw queueRejected('排队消息内容为空。');
  const images = [];
  const files = [];
  for (const block of blocks) {
    if (block?.type === 'text') {
      if (typeof block.text !== 'string') throw queueRejected('排队消息的文本块无效。');
      continue;
    }
    if (block?.type === 'image') {
      const mediaType = typeof block.mimeType === 'string' && QUEUE_IMAGE_TYPES.has(block.mimeType)
        ? block.mimeType : undefined;
      if (mediaType === undefined) throw queueRejected('图片仅支持 PNG、JPG、WebP、GIF 格式。');
      if (typeof block.data !== 'string' || !CANONICAL_BASE64.test(block.data)) {
        throw queueRejected('图片数据不是规范的 base64。');
      }
      const data = Buffer.from(block.data, 'base64');
      if (data.toString('base64') !== block.data) throw queueRejected('图片数据不是规范的 base64。');
      images.push({ data, mediaType });
      continue;
    }
    if (block?.type === 'file') {
      files.push(await stagedPromptFile(block, queueRejected));
      continue;
    }
    throw queueRejected('消息包含未知的附件类型。');
  }
  const attachments = ctx.get('attachments');
  let refs = [];
  if (images.length > 0) {
    const selection = agent.model.current();
    if (attachments === undefined || selection === undefined) {
      throw queueRejected('当前会话未准备好接收图片，请稍后重试。');
    }
    let info;
    try {
      info = await ctx.llm.resolveModelInfo(selection.provider, selection.model);
    } catch {
      throw queueRejected('无法确认当前模型是否支持图片。');
    }
    if (!Array.isArray(info?.inputModalities) || !info.inputModalities.includes('image')) {
      throw queueRejected(`模型“${selection.model}”不支持图片输入。`);
    }
    try {
      refs = await attachments.saveImages(images);
    } catch (error) {
      throw queueRejected(error instanceof Error ? error.message : String(error));
    }
  }
  const fileRefs = [];
  if (files.length > 0 && !attachments) throw queueRejected('当前服务没有可用的附件存储。');
  for (const file of files) {
    try {
      const attachment = await readStagedFile(file, (data, name) => attachments.saveFileStream({ data, name }));
      if (attachment.bytes !== file.size) throw new Error('附件长度与上传时不符。');
      fileRefs.push(attachment);
    } catch (error) {
      throw queueRejected(error instanceof Error ? error.message : String(error));
    }
  }
  const content = [];
  let pendingText = '';
  let imageIndex = 0;
  let fileIndex = 0;
  for (const block of blocks) {
    if (block.type === 'text') {
      pendingText += block.text;
      continue;
    }
    if (pendingText.length > 0) {
      content.push({ type: 'text', text: pendingText });
      pendingText = '';
    }
    if (block.type === 'file') content.push({ type: 'file', attachment: fileRefs[fileIndex++] });
    else content.push({ type: 'image', attachment: refs[imageIndex++] });
  }
  if (pendingText.length > 0) content.push({ type: 'text', text: pendingText });
  return content;
}

async function watchTerminals(ctx, { sessionId, request, active = true }) {
  const terminals = ctx.get('terminalController');
  if (!active || !terminals) {
    terminalWatches.get(sessionId)?.controller.abort();
    terminalWatches.delete(sessionId);
    return active ? { available: false } : [];
  }
  if (active !== true) {
    const watch = terminalWatches.get(sessionId);
    if (!watch) throw new SessionOperationError('terminal/unavailable', 'Agent 终端清单尚未连接。');
    watch.holds = Array.isArray(active)
      ? [...new Set(active.filter(id => typeof id === 'string'))]
      : [];
    watch.syncHolds();
    return watch.result;
  }
  const previous = terminalWatches.get(sessionId);
  if (previous?.request === request) return previous.result;
  previous?.controller.abort();
  const controller = new AbortController();
  const record = { request, controller, result: [], retained: new Map(), holds: [] };
  terminalWatches.set(sessionId, record);
  // The current controller publishes the authoritative roster as a stream.
  // Retain only terminals represented by open sidebar tabs, independently of
  // their output attachments; detach never releases that window hold.
  let iterator;
  try {
    const inventory = terminals.watch(sessionId, controller.signal);
    iterator = inventory[Symbol.asyncIterator]();
    const first = await iterator.next();
    if (first.done) throw new SessionOperationError('terminal/unavailable', 'Agent 终端清单已关闭。');
    record.result = first.value.map(terminalInfo);
  } catch (error) {
    if (terminalWatches.get(sessionId) === record) terminalWatches.delete(sessionId);
    controller.abort();
    void iterator?.return?.().catch(() => {});
    throw error;
  }
  const syncHolds = () => {
    const ids = new Set(record.holds);
    for (const [id, hold] of record.retained) {
      if (!ids.has(id)) {
        hold.abort();
        record.retained.delete(id);
      }
    }
    for (const id of ids) {
      if (record.retained.has(id)) continue;
      const hold = new AbortController();
      record.retained.set(id, hold);
      void (async () => {
        try {
          for await (const _frame of terminals.retain(sessionId, id, hold.signal)) {
            // The acknowledgement and stream lifetime are the window hold.
          }
        } catch {
          // Roster removal is authoritative; a stale hold must not reopen a tab.
        } finally {
          if (record.retained.get(id) === hold) record.retained.delete(id);
        }
      })();
    }
  };
  record.syncHolds = syncHolds;
  void (async () => {
    try {
      for (;;) {
        const next = await iterator.next();
        if (next.done || terminalWatches.get(sessionId) !== record) break;
        record.result = next.value.map(terminalInfo);
        syncHolds();
        process.stdout.write(JSON.stringify({ jsonrpc: '2.0', method: '_dsh/gpui/terminal-changed',
          params: { sessionId, terminals: record.result } }) + '\n');
      }
    } catch (error) {
      if (!controller.signal.aborted) {
        process.stderr.write(`Agent terminal roster: ${error instanceof Error ? error.message : String(error)}\n`);
      }
    } finally {
      if (terminalWatches.get(sessionId) === record) terminalWatches.delete(sessionId);
      controller.abort();
      for (const hold of record.retained.values()) hold.abort();
      record.retained.clear();
    }
  })();
  return record.result;
}

function terminalAttachmentKey(sessionId, terminalId) {
  return `${sessionId}\0${terminalId}`;
}

function terminalAttachment(ctx, sessionId, terminalId) {
  const attachment = terminalAttachments.get(terminalAttachmentKey(sessionId, terminalId));
  if (!attachment) throw new SessionOperationError('terminal/unavailable', '终端尚未连接或已关闭。');
  return attachment;
}

async function attachTerminal(ctx, { sessionId, terminalId }) {
  const key = terminalAttachmentKey(sessionId, terminalId);
  if (terminalAttachments.has(key)) throw new SessionOperationError('terminal/already-attached', '终端已连接。');
  const controller = ctx.get('terminalController');
  if (!controller) throw new SessionOperationError('terminal/unavailable', 'Agent 终端不可用。');
  const attachment = new AgentTerminalAttachment(controller, liveAgent(ctx, sessionId), terminalId, event => {
    process.stdout.write(JSON.stringify({ jsonrpc: '2.0', method: '_dsh/gpui/terminal-frame', params: event }) + '\n');
  });
  terminalAttachments.set(key, attachment);
  try {
    const reply = await attachment.start();
    void attachment.pump().finally(() => {
      if (terminalAttachments.get(key) === attachment) terminalAttachments.delete(key);
    });
    return reply;
  } catch (error) {
    terminalAttachments.delete(key);
    await attachment.close();
    throw error;
  }
}

async function terminalInput(ctx, { sessionId, terminalId, ...command }) {
  return terminalAttachment(ctx, sessionId, terminalId).command(command);
}

async function detachTerminal(ctx, { sessionId, terminalId }) {
  const key = terminalAttachmentKey(sessionId, terminalId);
  const attachment = terminalAttachments.get(key);
  terminalAttachments.delete(key);
  await attachment?.close();
  return { detached: true };
}

async function closeTerminal(ctx, { sessionId, terminalId }) {
  const controller = ctx.get('terminalController');
  if (!controller) throw new SessionOperationError('terminal/unavailable', 'Agent 终端不可用。');
  await detachTerminal(ctx, { sessionId, terminalId });
  await controller.close(liveAgent(ctx, sessionId), terminalId);
  terminalWatches.get(sessionId)?.syncHolds();
  return { closed: true };
}

async function renameTerminal(ctx, { sessionId, terminalId, title }) {
  const controller = ctx.get('terminalController');
  if (!controller) throw new SessionOperationError('terminal/unavailable', 'Agent 终端不可用。');
  await controller.rename(liveAgent(ctx, sessionId), terminalId, title);
  terminalWatches.get(sessionId)?.syncHolds();
  return { title };
}

/** Resolve one live agent for an extension request. */
function liveAgent(ctx, sessionId) {
  const agent = ctx.get('agents')?.get(sessionId);
  if (agent === undefined) throw new SessionOperationError('session/not-found', '会话不存在或尚未打开。');
  return agent;
}

async function composerPreference(ctx, requested) {
  const settings = ctx.get('settings');
  const read = () => settings?.describe().find(form => form.ns === 'ui-conversation');
  let form = read();
  if (!form) return { available: false };
  if (requested != null && requested !== 'queue' && requested !== 'steer') {
    throw new SessionOperationError('conversation/busy-enter', '不支持的发送行为。');
  }
  if (requested != null && requested !== form.value.busyEnter) {
    await settings.mutate('ui-conversation', [{ op: 'set', path: ['busyEnter'], value: requested }], form.revision);
    form = read();
  }
  return { available: true, mode: form?.value.busyEnter === 'steer' ? 'steer' : 'queue' };
}

function promptAttachmentRefs(content) {
  return content.flatMap(block => {
    if (!['image', 'file'].includes(block.type) || !block.attachment?.attachmentId) return [];
    const { attachmentId, mediaType, name, bytes } = block.attachment;
    return [{ type: block.type, attachmentId, mediaType, name, bytes }];
  });
}

function inboxRow(message, mode) {
  return {
    itemId: message.id,
    mode,
    text: contentBlocks(message.content),
    editText: message.content.every(block => block.type === 'text')
      ? message.content.map(block => block.text).join('') : null,
    hasImage: message.content.some(block => block.type === 'image'),
    attachments: promptAttachmentRefs(message.content),
    sourceKind: message.source?.kind ?? 'user',
    requestId: message.source?.kind === 'user' ? message.source.rpcId : undefined,
  };
}

/** Pending Inbox rows in display order: queued turns first, then next-step steering. */
function inboxRows(ctx, agent) {
  let state;
  try {
    state = ctx.get('sessionProjections')?.snapshot(agent.session, ['inbox']).values.inbox;
  } catch {
    state = undefined;
  }
  const queued = state?.['next-turn'] ?? agent.inbox.nextTurn;
  const steering = state?.['next-step'] ?? agent.inbox.nextStep;
  return [
    ...queued.map(message => inboxRow(message, 'queue')),
    ...steering.map(message => inboxRow(message, 'steer')),
  ];
}

async function queuePrompt(ctx, params) {
  const agent = liveAgent(ctx, params.sessionId);
  const content = await queuePromptContent(ctx, agent, params.prompt);
  const { createUserMessage } = await llmApi();
  const source = typeof params.requestId === 'string' && params.requestId
    ? { kind: 'user', rpcId: params.requestId }
    : { kind: 'user' };
  const message = createUserMessage({ content, source });
  if (ctx.get('agents')?.get(params.sessionId) !== agent) {
    throw new SessionOperationError('session/not-found', '上传期间会话已关闭，草稿已保留。');
  }
  // Steering only owns the running turn; an idle agent takes the same message
  // as the next turn instead of losing it (official `resolveSubmitMode`).
  const steering = params.mode === 'steer' && agent.status === 'running';
  if (steering) agent.steer(message);
  else agent.followup(message);
  return inboxRow(message, steering ? 'steer' : 'queue');
}

function inboxState(ctx, params) {
  const agent = liveAgent(ctx, params.sessionId);
  // Echo the client's request sequence: a snapshot taken before a newer
  // submission must not delete the row that submission added.
  const requestSeq = Number.isSafeInteger(params.requestSeq) ? params.requestSeq : 0;
  return { requestSeq, running: agent.status === 'running', items: inboxRows(ctx, agent) };
}

async function inboxAction(ctx, params) {
  const freezeMessage = params.action === 'edit' ? (await llmApi()).freezeMessage : undefined;
  const agent = liveAgent(ctx, params.sessionId);
  const queued = agent.inbox.nextTurn.find(message => message.id === params.itemId);
  const steering = agent.inbox.nextStep.find(message => message.id === params.itemId);
  if (queued === undefined && steering === undefined) {
    throw new SessionOperationError('session/queue-item', '排队消息已不在队列中。');
  }
  if (params.action === 'edit') {
    if (queued === undefined || !queued.content.every(block => block.type === 'text')) {
      throw queueRejected('包含非文本内容，暂不支持编辑。');
    }
    if (typeof params.text !== 'string' || params.text.trim() === '') {
      throw queueRejected('排队消息内容为空。');
    }
    const edited = freezeMessage({ ...queued, content: [{ type: 'text', text: params.text }] });
    if (!agent.inbox.replace(params.itemId, edited)) {
      throw new SessionOperationError('session/queue-item', '排队消息已不在队列中。');
    }
    return { item: inboxRow(edited, 'queue') };
  }
  if (params.action === 'remove') {
    return { removed: agent.inbox.remove(params.itemId) };
  }
  if (params.action === 'steer') {
    // Official `session/steer-unavailable`: only a queued next-turn row can
    // join a still-running turn.
    if (queued === undefined || agent.status !== 'running') {
      throw new SessionOperationError('session/steer-unavailable', '当前回合已结束，无法插话发送。');
    }
    agent.inbox.remove(params.itemId);
    agent.steer(queued);
    return { steered: true };
  }
  throw new SessionOperationError('session/queue-item', '不支持的排队消息操作。');
}

/** Current live goal view, or null when no goal is set. */
function currentGoalView(ctx, agent) {
  const goals = ctx.get('goals');
  if (goals === undefined) return null;
  try {
    return goals.get(agent) ?? null;
  } catch {
    // The agent may have been disposed between the event and this read.
    return null;
  }
}

let goalSequence = 0;

function goalSnapshot(ctx, agent) {
  return { goal: currentGoalView(ctx, agent), sequence: ++goalSequence };
}

function goalState(ctx, params) {
  return goalSnapshot(ctx, liveAgent(ctx, params.sessionId));
}

function goalAction(ctx, params) {
  const goals = ctx.get('goals');
  if (goals === undefined) {
    throw new SessionOperationError('session/goal-unavailable', '当前服务未启用目标功能。');
  }
  const agent = liveAgent(ctx, params.sessionId);
  const current = goals.get(agent);
  if (current === undefined) {
    throw new SessionOperationError('session/goal-missing', '当前会话没有进行中的目标。');
  }
  const { goalId, revision } = params;
  if (typeof goalId !== 'string' || goalId.length === 0 || !Number.isSafeInteger(revision)) {
    throw new SessionOperationError('session/goal-invalid', '缺少目标身份或版本。');
  }
  const ref = { id: goalId, revision };
  switch (params.action) {
    case 'pause': goals.pause(agent, ref); break;
    case 'resume': goals.resume(agent, ref); break;
    case 'clear': goals.clear(agent, ref); break;
    case 'edit': {
      if (typeof params.objective !== 'string' || params.objective.trim().length === 0) {
        throw new SessionOperationError('session/goal-invalid', '目标内容不能为空。');
      }
      goals.edit(agent, ref, { objective: params.objective.trim() });
      break;
    }
    default:
      throw new SessionOperationError('session/goal-invalid', '不支持的目标操作。');
  }
  return goalSnapshot(ctx, agent);
}

/** Read or save Team mode through the root Session's actual Cordis services. */
async function teamState(ctx, params) {
  const request = Number.isSafeInteger(params.request) ? params.request : null;
  const reply = (result) => ({ ...result, request });
  const agent = liveAgent(ctx, params.sessionId);
  const projections = ctx.get('sessionProjections');
  if (agent.session.header.origin === 'subagent' || !ctx.get('agentTeams') || !projections) {
    return reply({ available: false });
  }
  const read = () => {
    const { values } = projections.snapshot(agent.session, ['agentTeam', 'modelSelection']);
    return values.agentTeam === undefined ? null : {
      ...values.agentTeam,
      hasUsedModel: values.modelSelection === undefined ? null : values.modelSelection.lastUsed !== null,
    };
  };
  // Tauri serializes an omitted Option<bool> as null in its ACP envelope.
  if (params.enabled == null) return reply({ available: true, team: read() });
  if (typeof params.enabled !== 'boolean') {
    throw new SessionOperationError('team/invalid-selection', '智能体团队协作设置无效。');
  }
  // setEnabled enforces root ownership, appends the durable team/mode event and
  // flushes the owning Session. A successful flush can still change the view
  // synchronously before this reply, so always return a fresh authoritative cut.
  await ctx.get('agentTeams').setEnabled(agent, params.enabled);
  return reply({ available: true, team: read() });
}

async function registerSessionListMetadata(ctx) {
  const projections = ctx.get('sessionProjections');
  const key = 'sessionListMetadata';
  if (!projections || projections.summaryKeys().has(key)) return;
  const manager = createRequire(require.resolve('@deepseek-ai/dsh-plugin-manager'));
  const { z } = await import(pathToFileURL(manager.resolve('zod')).href);
  const schema = z.object({ blank: z.boolean(), lastPromptAt: z.number().nullable() });
  // The ACP profile does not mount ApiSessionList. Persist the same row its
  // Workspace UI reads so KWork and Harness can share the projection cache.
  await projections.register({
    key,
    stateSchema: schema,
    init: () => ({ blank: true, lastPromptAt: null }),
    apply: (state, event) => {
      const blank = state.blank && event.type !== 'turn/start';
      const lastPromptAt = event.type === 'user/message' && event.data.source?.kind === 'user'
        ? event.time : state.lastPromptAt;
      return blank === state.blank && lastPromptAt === state.lastPromptAt ? state : { blank, lastPromptAt };
    },
    wire: { viewSchema: schema, view: state => state },
    stateVersion: 1,
  });
}

function observeSessionUpdates(ctx) {
  observePlanReviews(ctx);
  // The registered Cordis projection is authoritative for both saved mode and
  // task readiness. Never approximate its graph or persistence checks here.
  ctx.get('sessionProjections')?.onChanged((session, key, value) => {
    if (key === 'agentTeam' || key === 'modelSelection') {
      notify(session.id, { sessionUpdate: 'gpui_team_changed' });
    }
  });
  const notifyAgentStatus = (agent, running) => {
    const { origin, parentSession } = agent.session.header;
    notify(agent.id, {
      sessionUpdate: 'gpui_agent_status', running,
      ...(origin === 'subagent' && parentSession ? { parentSessionId: parentSession } : {}),
    });
  };
  ctx.on('agent/status', ({ agent, status }) => notifyAgentStatus(agent, status === 'running'), { global: true });
  ctx.on('agent/disposed', ({ agent }) => notifyAgentStatus(agent, false), { global: true });
  const publishGoal = agent => notify(agent.id, {
    sessionUpdate: 'gpui_goal_changed',
    ...goalSnapshot(ctx, agent),
  });
  ctx.on('goal/changed', ({ agent }) => publishGoal(agent), { global: true });
  ctx.on('goal/activation-changed', ({ sessionId }) => {
    const agent = ctx.get('agents')?.get(sessionId);
    if (agent !== undefined) publishGoal(agent);
  }, { global: true });
  ctx.on('agent/assistant-stream', ({ agent, frame }) => {
    if (frame.type === 'chunk') {
      const chunk = frame.chunk;
      if (chunk.type === 'block-start' && chunk.blockType === 'tool-call') {
        notify(agent.id, { sessionUpdate: 'gpui_tool_preparing', preparing: true, name: null });
        return;
      }
      if (chunk.type === 'tool-call-delta') {
        const previous = preparingTools.get(agent);
        if (chunk.name && chunk.name !== previous) {
          preparingTools.set(agent, chunk.name);
          notify(agent.id, { sessionUpdate: 'gpui_tool_preparing', preparing: true, name: chunk.name });
        }
        return;
      }
      if (chunk.type === 'block-end' && chunk.block?.type === 'tool-call') {
        preparingTools.delete(agent);
        notify(agent.id, { sessionUpdate: 'gpui_tool_preparing', preparing: false });
        return;
      }
      // Token deltas need no forwarding: the client's stream-preview rows are
      // disabled (they froze the transcript), and durable ACP content owns the
      // transcript text. Only the prepare signal is published from this stream.
      return;
    }
    if (frame.type === 'end') {
      // The message finished streaming: the tool now executes, and its live
      // activity label takes over from 准备X.
      preparingTools.delete(agent);
      notify(agent.id, { sessionUpdate: 'gpui_tool_preparing', preparing: false });
    }
  }, { global: true });
  ctx.on('session/event', (session, event) => {
    if (event.type === 'tool/ptc-dispatch-start' || event.type === 'tool/ptc-dispatch'
      || event.type.startsWith('tool-workflow/')) {
      notify(session.id, { sessionUpdate: 'gpui_ptc_dispatch', event });
    }
  }, { global: true });
  ctx.on('session/event', (session, event) => {
    if (event.type !== 'agent/inbox/spliced') return
    const data = event.data ?? {}
    let state = inboxes.get(session)
    if (!state) inboxes.set(session, state = { queues: { 'next-turn': [], 'next-step': [] }, claimed: new Set() })
    const queue = state.queues[data.target === 'next-step' ? 'next-step' : 'next-turn']
    const start = Number.isSafeInteger(data.start) ? data.start : 0
    const removed = Number.isSafeInteger(data.removedCount) ? data.removedCount : 0
    for (const message of queue.slice(start, start + removed)) state.claimed.add(message.id)
    queue.splice(start, removed, ...(Array.isArray(data.inserted) ? data.inserted : []))
  }, { global: true });
  ctx.on('session/event', (session, event) => {
    const { type, data, surfaceOp } = event;
    if (type === 'plan/mode') {
      notify(session.id, { sessionUpdate: 'gpui_plan_mode', active: data.active });
      return;
    }
    if (type === 'user/message') {
      const source = data.source?.kind;
      if (source === 'user') {
        notify(session.id, { sessionUpdate: 'gpui_session_activity', time: event.time });
        // The transcript row for a human message is durable: it is logged when
        // the Inbox claims it, not when the client submits it. Forwarding the
        // logged message lets the client reconcile its optimistic row, and is
        // the only live source for a steering message admitted mid-turn.
        const images = (Array.isArray(data.content) ? data.content : [])
          .filter(block => block?.type === 'image' && block.attachment !== undefined)
          .map(block => ({
            attachmentId: block.attachment.attachmentId,
            mediaType: block.attachment.mediaType,
            name: block.attachment.name,
            width: block.attachment.width,
            height: block.attachment.height,
          }))
        notify(session.id, {
          sessionUpdate: 'gpui_user_message',
          messageId: data.id,
          requestId: source === 'user' ? data.source.rpcId : undefined,
          content: contentBlocks(data.content),
          hasImage: images.length > 0,
          images,
          files: promptAttachmentRefs(Array.isArray(data.content) ? data.content : []).filter(block => block.type === 'file'),
          time: event.time,
          seq: event.seq,
        });
      }
      const human = source === 'user' || source === 'user-question-reply';
      if (!human) {
        // Injected context (time snapshots, runtime snapshots, catalogs, tool
        // notices): a collapsed row, never a trigger card and never a model
        // prompt bubble. Sending these as triggers put a giant
        // "Time sampled while preparing…" bubble into every live step while a
        // reopened session dropped them.
        if (source === 'compact-checkpoint' && typeof data.source?.compactionId === 'string') {
          // The checkpoint text is hidden model context; the official transcript
          // shows the compaction marker at this seq instead.
          const compactionId = data.source.compactionId
          const cached = compactionSummaries.get(compactionId) ?? {}
          notify(session.id, {
            sessionUpdate: 'gpui_compaction',
            compactionId,
            summary: cached.summary ?? '',
            items: cached.items ?? null,
            tokens: cached.tokens ?? null,
            time: event.time,
          })
          return
        }
        // Claimed by the inbox for this turn → waking trigger; otherwise a
        // mid-turn injection keeps the context row.
        const waking = inboxes.get(session)?.claimed.has(data.id) === true;
        if (!waking) {
          notify(session.id, {
            sessionUpdate: 'context_injection',
            messageId: data.id,
            sourceKind: source,
            // Full durable source so the client can present the form-specific
            // body (sections, summary, changes, sender, references) with the same
            // data a reopened session has in its log.
            source: data.source,
            form: data.source?.form,
            entries: data.source?.entries,
            update: data.source?.update === true,
            incomplete: data.source?.incomplete === true,
            content: contentBlocks(data.content),
            time: event.time,
          });
          return;
        }
        // Waking notifications keep their trigger card (official
        // TurnTriggerNodeView) so a goal round or job settle is visible. The
        // variant facts (subagent stop reason, webhook provider) select the
        // card's localized title.
        notify(session.id, {
          sessionUpdate: 'turn_trigger',
          messageId: data.id,
          sourceKind: source,
          form: data.source?.form,
          stopReason: data.source?.stopReason,
          provider: data.source?.provider,
          content: contentBlocks(data.content),
          time: event.time,
        });
      }
      if (['user', 'goal', 'schedule', 'webhook', 'github', 'agent', 'agent-message'].includes(source) ||
          (source === 'user-question-reply' && data.source.outcome === 'answered')) {
        notify(session.id, { sessionUpdate: 'gpui_turn_tail', type, seq: event.seq });
      }
      return;
    }
    // Command results are log-only session events (`command/run` then
    // `command/done`). The official client turns them into a command row in the
    // transcript, so they must reach the UI instead of being dropped here.
    if (type === 'command/run' || type === 'command/done') {
      notify(session.id, {
        sessionUpdate: type === 'command/run' ? 'gpui_command_run' : 'gpui_command_done',
        seq: event.seq,
        time: event.time,
        commandId: data.commandId,
        name: data.name,
        args: data.args,
        kind: data.kind,
        text: data.text,
      });
      return;
    }
    // The complete model-visible system prompt is a collapsed disclosure row in
    // the official transcript (SystemPromptRow), so forward its text once.
    if (type === 'system/message') {
      notify(session.id, {
        sessionUpdate: 'gpui_system_prompt',
        seq: event.seq,
        text: contentBlocks(data.message?.content),
        update: surfaceOp !== undefined && surfaceOp !== 'append',
        time: event.time,
      });
      return;
    }
    // Files delivered by `present` are announced as a durable event; the official
    // DeliverablesTail turns the validated list into file cards at the turn tail.
    if (type === 'deliverables/presented') {
      const files = Array.isArray(data.files) ? data.files : []
      if (files.length > 0) {
        notify(session.id, {
          sessionUpdate: 'gpui_deliverables',
          callId: data.callId,
          files,
          time: event.time,
        })
      }
      return
    }
    // A compaction summary is evidence for the marker, not a row of its own.
    if (type === 'compaction/summary') {
      const compactionId = data.compactionId
      const text = contentBlocks(data.summary)
      const items = Array.isArray(data.shadowedSeqs) ? data.shadowedSeqs.length : null
      const tokens = Number.isSafeInteger(data.shadowedTokenCount) ? data.shadowedTokenCount : null
      if (typeof compactionId === 'string' && compactionId !== '') {
        compactionSummaries.set(compactionId, { summary: text, items, tokens })
        notify(session.id, {
          sessionUpdate: 'gpui_compaction_summary',
          compactionId,
          summary: text,
          items,
          tokens,
          time: event.time,
        })
      }
      return
    }
    // Tool-registry notices are `developer/message` events whose content is a
    // list of tool additions/removals. The official transcript renders them as
    // ContextInjectionRow variants (已添加工具：… / 工具已更新 + 新增/移除 lists);
    // without this they never reach the UI at all.
    if (type === 'developer/message') {
      const source = data.source?.kind ?? data.message?.source?.kind;
      if (source !== 'tool-registry') return;
      const changes = (Array.isArray(data.content) ? data.content : [])
        .filter(block => block?.type === 'tool-addition' || block?.type === 'tool-removal')
        .map(block => ({ added: block.type === 'tool-addition', name: block.toolName }));
      if (changes.length === 0) return;
      notify(session.id, {
        sessionUpdate: 'context_injection',
        messageId: data.message?.id ?? `tool-registry-${event.seq}`,
        sourceKind: 'tool-registry',
        toolChanges: changes,
        time: event.time,
      });
      return;
    }
    // Direct-child discovery facts are what the official lineage catalog lists;
    // the parent run can open each child's transcript from them.
    if (type === 'subagent/catalog') {
      notify(session.id, { sessionUpdate: 'gpui_subagent_catalog', ...data });
      return;
    }
    // Every pending-Inbox mutation is durable and already carries its whole
    // splice. The client re-reads the pending rows instead of rebuilding the
    // splice chain itself, so a claimed steering item leaves the dock exactly
    // when the Host admits it.
    if (type === 'agent/inbox/spliced') {
      notify(session.id, { sessionUpdate: 'gpui_inbox_changed', target: data.target });
      return;
    }
    // Background job lifecycle. `job/registered` opens a row and `job/settled`
    // closes the same one, keyed by jobId; the durable events carry the kind,
    // label and terminal status the transcript needs.
    if (type === 'job/registered' || type === 'job/settled') {
      notify(session.id, {
        sessionUpdate: type === 'job/registered' ? 'gpui_job_registered' : 'gpui_job_settled',
        jobId: data.jobId,
        kind: data.kind,
        label: data.label,
        startedAt: data.startedAt,
        finishedAt: data.finishedAt,
        status: data.status,
        detail: data.detail,
        time: event.time,
      });
      return;
    }
    const relevant = ['session/title', 'turn/start', 'step/start', 'step/end', 'turn/end', 'llm/retry',
      'request/header', 'request/context', 'assistant/attempt', 'tool/ptc-dispatch-start', 'tool/ptc-dispatch',
      'compaction/start', 'compaction/end', 'compaction/summary', 'developer/message', 'system/message'].includes(type)
      || ((type === 'assistant/message' || type === 'tool/result' || type === 'user/message') && surfaceOp === 'append');
    if (!relevant) return;
    notify(session.id, { sessionUpdate: 'gpui_trajectory_event', event });
    const update = type === 'session/title' ? {
      sessionUpdate: 'gpui_session_title', seq: event.seq, title: data.title,
    } : {
      sessionUpdate: 'gpui_turn_tail', type, seq: event.seq, turn: data.turn, step: data.step, time: event.time,
    };
    if (type === 'tool/call') {
      update.toolCall = {
        sessionUpdate: 'tool_call', toolCallId: data.callId,
        title: data.name, rawInput: data.arguments, status: 'in_progress', time: event.time,
      };
    }
    if (type === 'tool/result') {
      update.callId = data.message.source?.callId ?? data.message.toolCallId;
      // The same metadata and attachment references are in session.v4; preserve
      // them so live and replayed diff/image cards are identical.
      if (data.meta !== undefined) update.meta = data.meta;
      if (Array.isArray(data.message.content)
        && data.message.content.some(block => block?.type === 'image')) {
        update.toolContent = data.message.content;
      }
    }
    if (type === 'llm/retry' || type === 'llm/retry-started') {
      // The retry row shows the attempt, the countdown and (expanded) the delay
      // and failure, so the facts travel with the update instead of only the id.
      notify(session.id, {
        sessionUpdate: 'model_retry',
        retryId: data.retryId,
        retry: data.retry,
        maximum: data.maxRetries,
        retryState: type === 'llm/retry-started' ? 'started' : 'scheduled',
        delayMs: data.delayMs,
        failure: data.failure,
      });
      return;
    }
    if (type === 'turn/end') {
      update.reason = data.reason?.kind;
      // The client renders the terminal failure row (official TurnErrorItem),
      // which needs the recorded code and message, not just the end kind.
      update.reasonObject = data.reason;
      const plans = submittedPlanEvents(session, event.seq);
      if (plans.length > 0) update.planEvents = plans;
    }
    if (type === 'assistant/message') {
      const { id, content } = data.message;
      update.assistantId = id;
      update.hasToolCalls = content.some(block => block.type === 'tool-call');
      update.hasReply = content.some(block => block.type === 'text'
        ? block.text.trim() !== '' : block.type !== 'tool-call' && block.type !== 'reasoning');
      update.hasVisibleContent = update.hasReply || content.some(block =>
        block.type === 'reasoning' && block.text.trim() !== '');
      if (content.some(block => block.type === 'text' && block.text.trim() !== '')) update.messageId = id;
    }
    notify(session.id, update);
  });
}

const searchResultLimit = 20;
const searchMessageTypes = ['user/message', 'assistant/message'];
let latestSessionSearch;

async function searchSessions(ctx, { query, generation } = {}) {
  const empty = { items: [], hasMore: false };
  if (!Number.isSafeInteger(generation) || generation < 1) {
    throw new SessionOperationError('session/search', 'Invalid search generation');
  }
  if (generation <= (latestSessionSearch?.generation ?? 0)) return empty;
  latestSessionSearch?.controller.abort();
  const controller = new AbortController();
  latestSessionSearch = { generation, controller };
  if (query === null) return empty;
  if (typeof query !== 'string' || !query.trim() || query.length > 500 || query.includes('\0')) {
    throw new SessionOperationError('session/search', 'Invalid search query');
  }
  const provider = ctx.get('sessionQuery');
  if (!provider) return empty;
  const { signal } = controller;
  try {
    const records = await provider.listSessions(signal);
    signal.throwIfAborted();
    const visibleIds = new Set(records.filter(record => record.header.cwd !== undefined)
      .map(record => record.header.id));
    if (visibleIds.size === 0) return empty;

    const items = new Map();
    const seenCursors = new Set();
    let cursor;
    let limit = searchResultLimit;
    for (let calls = 0; calls < 100; calls++) {
      let page;
      try {
        page = await provider.searchSessions({
          query: query.trim(),
          eventFilters: [
            { kind: 'type', values: searchMessageTypes },
            { kind: 'surface', values: ['current'] },
          ],
          limit,
          ...(cursor === undefined ? {} : { cursor }),
        }, { signal });
      } catch (error) {
        signal.throwIfAborted();
        if (cursor === undefined && error?.code === 'SESSION_QUERY_INVALID_LIMIT' && limit > 1) {
          limit = Math.floor(limit / 2);
          continue;
        }
        if (cursor !== undefined && error?.code === 'SESSION_QUERY_STALE_CURSOR') {
          items.clear();
          seenCursors.clear();
          cursor = undefined;
          continue;
        }
        throw error;
      }
      signal.throwIfAborted();
      if (page.items.length > limit) throw new Error('Session search provider exceeded page limit');
      for (const { header, bestMatch } of page.items) {
        const sessionId = header.id;
        if (!visibleIds.has(sessionId) || items.has(sessionId) || bestMatch.sessionId !== sessionId
          || bestMatch.surface !== 'current' || !searchMessageTypes.includes(bestMatch.type)) continue;
        if (items.size === searchResultLimit) return { items: [...items.values()], hasMore: true };
        items.set(sessionId, { sessionId, snippet: Array.from(bestMatch.snippet).slice(0, 240).join('') });
      }
      cursor = page.nextCursor;
      if (cursor === undefined) return { items: [...items.values()], hasMore: false };
      if (seenCursors.has(cursor)) throw new Error('Session search provider repeated a continuation cursor');
      seenCursors.add(cursor);
    }
    throw new Error('Session search provider exceeded its work budget');
  } catch (error) {
    if (signal.aborted || error?.code === 'SESSION_QUERY_SEARCH_DISABLED') return empty;
    throw error;
  }
}

async function extension({ id, method, params }) {
  let response;
  try {
    const { ctx } = await ready;
    if (method === openHostSessionMethod) {
      const sessions = ctx.get('acpSessionHost');
      if (typeof params?.sessionId !== 'string' || !params.sessionId || typeof params.cwd !== 'string') {
        throw new SessionOperationError('session/activate', '缺少会话 ID 或工作区路径。');
      }
      try {
        const opened = sessions ? await sessions.openForClient(params.sessionId, params.cwd) : null;
        response = { result: opened && {
          ...opened,
          hostRunning: ctx.get('agents')?.get(params.sessionId)?.status === 'running',
        } };
      } catch (error) {
        throw new SessionOperationError('session/activate', error instanceof Error ? error.message : String(error));
      }
    } else if (method === sessionArchiveMethod) {
      response = { result: await enqueueWorkspaceOperation(() => sessionArchive(ctx, params)) };
    } else if (method === workspaceRegistryMethod) {
      response = { result: await enqueueWorkspaceOperation(() => workspaceRegistryRequest(ctx, params?.operation)) };
    } else if (method === searchSessionsMethod) {
      response = { result: await searchSessions(ctx, params) };
    } else if (method === modelCatalogMethod) {
      response = { result: await readModelCatalog(ctx) };
    } else if (method === deepSeekAccountMethod) {
      response = { result: await deepSeekAccountOperation(ctx, params ?? {}) };
    } else if (method === accountNotificationsMethod) {
      response = { result: await accountNotifications(ctx, params ?? {}) };
    } else if (method === platformSessionMethod) {
      // Private stdio response consumed by the desktop process. Account UI
      // commands return only accountWireState and never forward this snapshot.
      response = { result: await readPlatformSession() };
    } else if (method === sessionReadStateMethod) {
      response = { result: await sessionReadState(ctx, params) };
    } else if (method === scheduleMethod) {
      response = { result: await scheduleRequest(ctx, params?.operation) };
    } else if (method === runtimeSettingsMethod) {
      response = { result: await runtimeSettings(ctx, params) };
    } else if (method === webSettingsMethod) {
      response = { result: await webSettings(ctx, params) };
    } else if (method === composerPreferenceMethod) {
      response = { result: await composerPreference(ctx, params?.mode) };
    } else if (method === pluginCenterMethod) {
      response = { result: await pluginCenterRequest(ctx, params) };
    } else if (method === pluginInstallMethod) {
      response = { result: await pluginInstallRequest(ctx, params?.operation) };
    } else if (modelProviderMethods.has(method)) {
      response = { result: await modelProviderOperation(ctx, method, params ?? {}) };
    } else if (method === questionStateMethod) {
      await startQuestions(ctx);
      const sessionId = params?.sessionId;
      const rows = typeof sessionId === 'string'
        ? publishContinuedQuestions(ctx, ctx.get('agents').get(sessionId), true) : [];
      response = { result: rows };
    } else if (typeof params?.sessionId !== 'string' || params.sessionId.length === 0) {
      response = { error: { code: -32602, message: 'Invalid session id' } };
    } else if (method === terminalWatchMethod) {
      response = { result: await watchTerminals(ctx, params) };
    } else if (method === terminalInputMethod) {
      response = { result: await terminalInput(ctx, params) };
    } else if (method === terminalAttachMethod) {
      response = { result: await attachTerminal(ctx, params) };
    } else if (method === terminalDetachMethod) {
      response = { result: await detachTerminal(ctx, params) };
    } else if (method === terminalCloseMethod) {
      response = { result: await closeTerminal(ctx, params) };
    } else if (method === terminalRenameMethod) {
      response = { result: await renameTerminal(ctx, params) };
    } else if (method === queuePromptMethod) {
      response = { result: await queuePrompt(ctx, params) };
    } else if (method === inboxStateMethod) {
      response = { result: inboxState(ctx, params) };
    } else if (method === inboxActionMethod) {
      response = { result: await inboxAction(ctx, params) };
    } else if (method === goalStateMethod) {
      response = { result: goalState(ctx, params) };
    } else if (method === goalActionMethod) {
      response = { result: goalAction(ctx, params) };
    } else if (method === teamStateMethod) {
      response = { result: await teamState(ctx, params) };
    } else if (method === recordMethod) {
      const feedback = ctx.get('sessionFeedback');
      response = feedback
        ? { result: await feedback.record(params) }
        : { error: { code: -32601, message: 'Session feedback is unavailable' } };
    } else if (method === flushMethod) {
      const sessions = ctx.get('sessions');
      const session = sessions?.get(params.sessionId);
      if (session) await sessions.flush(session);
      response = { result: { live: session !== undefined } };
    } else if (method === planReviewMethod) {
      response = { result: respondPlanReview(params) };
    } else if (method === planModeMethod) {
      response = { result: planMode(ctx, params) };
    } else if (method === permissionModeMethod) {
      response = { result: await permissionMode(ctx, params) };
    } else if (method === sessionCommandsMethod) {
      response = { result: sessionCommands(ctx, params) };
    } else if (method === executeSessionCommandMethod) {
      response = { result: await executeSessionCommand(ctx, params) };
    } else if (method === questionDecisionMethod) {
      response = { result: respondQuestion(ctx, params) };
    } else if (method === forkMethod || method === renameMethod) {
      const mutate = method === forkMethod ? forkSession : renameSession;
      const task = sessionMutationTail.then(() => mutate(ctx, params));
      sessionMutationTail = task.then(() => undefined, () => undefined);
      response = { result: await task };
    } else {
      const feedback = await messageFeedback(ctx);
      response = feedback
        ? { result: await feedback[messageMethods.get(method)](params) }
        : { error: { code: -32601, message: 'Message feedback is unavailable' } };
    }
  } catch (error) {
    response = { error: error instanceof SessionOperationError
      ? { code: -32602, message: error.message, ...(error.message === error.code ? {} : { data: error.code }) }
      : { code: -32603, message: 'Session operation failed', data: error instanceof Error ? error.message : String(error) } };
  }
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, ...response }) + '\n');
}

async function forward() {
  try {
    for (;;) {
      if (!inputLines.length && !inputEnded) {
        await new Promise(resolve => { inputWake = resolve; });
        inputWake = undefined;
      }
      const line = inputLines.shift();
      if (line === undefined) break;
      if (extensionMethods.some(method => line.includes(method))) {
        let request;
        try { request = JSON.parse(line); } catch { /* ACP owns ordinary protocol errors. */ }
        if (request?.id != null && extensionMethods.includes(request.method)) {
          void extension(request);
          continue;
        }
      }
      if (!acpInput.write(line + '\n')) await once(acpInput, 'drain');
    }
  } finally {
    acpInput.end();
  }
}

try {
  // Start consuming before awaiting the profile. An early `initialize` must be
  // forwarded to the ACP transport rather than left in a dormant iterator.
  const forwarding = forward();
  await ready;
  await forwarding;
} catch (error) {
  if (error instanceof StartupError) {
    const extension = profileEntry.endsWith('.ts') ? 'ts' : 'js';
    const { reportStartupFailure } =
      await import(pathToFileURL(join(dirname(profileEntry), 'startup-diagnostics.' + extension)).href);
    const { resolveDshHome } = await import(pathToFileURL(require.resolve('@deepseek-ai/dsh-home-paths')).href);
    await reportStartupFailure(error, { home: resolveDshHome(), version: getDshRuntimeVersion(), profile: 'acp' });
  } else {
    console.error('dsh-gpui ACP bridge:', error);
  }
  process.exit(1);
} finally {
  platformSessionSource?.lifetime.abort();
  for (const attempt of providerAuthAttempts.values()) attempt.controller.abort();
  const account = await accountLoginService?.getState().catch(() => null);
  if (account?.attempt && ['initializing', 'waiting-browser', 'exchanging', 'committing'].includes(account.attempt.phase)) {
    await accountLoginService.cancelSignIn(account.attempt.id).catch(() => {});
  }
  await accountCallbackFiber?.dispose().catch(() => {});
}
