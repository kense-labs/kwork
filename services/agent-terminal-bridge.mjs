import { EventEmitter, once } from 'node:events';
import { lstat, mkdir, readFile, readlink, realpath, rename, rm, symlink } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';

async function packageAt(require, dependency, version) {
  for (const path of require.resolve.paths(dependency) ?? []) {
    const dir = join(path, dependency);
    try {
      const manifest = JSON.parse(await readFile(join(dir, 'package.json'), 'utf8'));
      if (manifest.name === dependency && (!version || manifest.version === version)) {
        return { dir: await realpath(dir), manifest };
      }
    } catch (error) {
      if (error?.code !== 'ENOENT') throw error;
    }
  }
}

async function linkTerminalPackage(home, source) {
  const links = [join(home, 'profiles/acp/node_modules', source.manifest.name)];
  if (source.manifest.name === '@deepseek-ai/dsh-api-terminal-controller') {
    // Node follows the linked panel to its real path. Keep the controller next
    // to the panel's runtime dependencies as well as in the profile mapping.
    links.push(join(dirname(dirname(source.dir)), 'node_modules', source.manifest.name));
  }
  for (const link of links) await linkPackage(link, source);
}

async function linkPackage(link, source) {
  await mkdir(dirname(link), { recursive: true });
  try {
    await symlink(source.dir, link, 'dir');
    return;
  } catch (error) {
    if (error?.code === 'EEXIST') {
      try {
        if ((await lstat(link)).isSymbolicLink() && await realpath(link) === source.dir) return;
      } catch (lookup) {
        if (lookup?.code !== 'ENOENT') throw lookup;
      }
      // Only an owned link can be replaced after the runtime's atomic rename.
      // A regular directory, another application's link, or an unrelated
      // package remains untouched and produces an actionable error instead.
      const existing = await lstat(link);
      const target = existing.isSymbolicLink() ? await readlink(link) : '';
      const ownedSource = source.dir.includes('/.kwork-runtime/') || source.dir.includes('/kwork-runtime/');
      const ownedTarget = target.includes('/.install-') || target.includes('/.kwork-runtime/')
        || target.includes('/kwork-runtime/') || target.includes('/kwork-terminal-runtime-');
      if (!existing.isSymbolicLink() || !ownedSource || !ownedTarget) {
        throw new Error(`ACP profile 已包含不同来源的 ${source.manifest.name}，未覆盖它。`);
      }
      const replacement = `${link}.kwork-${process.pid}`;
      await symlink(source.dir, replacement, 'dir');
      await rename(replacement, link).catch(async error => {
        await rm(replacement, { force: true });
        throw error;
      });
      return;
    }
    throw error;
  }
}

/**
 * Register the controller before runProfile computes its package interception.
 * Its runtime table then covers the controller and every transitive dependency;
 * no package is imported through an unmapped temporary path.
 */
export async function prepareTerminalProfile({ service, module, home, runtime }) {
  const manifest = JSON.parse(await readFile(join(dirname(module), '../package.json'), 'utf8'));
  const require = createRequire(join(service, 'apps/cli/package.json'));
  const anchors = [
    join(service, 'apps/cli/package.json'),
    join(service, 'packages/bundle/base/package.json'),
    join(service, 'packages/api/gateway/package.json'),
    join(service, 'packages/subprocess/subprocess-local/package.json'),
    ...(runtime ? [join(runtime, 'package.json')] : []),
  ];
  const resolved = new Map();
  const controller = { dir: await realpath(dirname(dirname(module))), manifest };
  // Optional consumers declare the controller as a peer, so its own package
  // must already be resolvable while their dependency closure is scanned.
  resolved.set(manifest.name, controller);
  const packageOverrides = {
    '@deepseek-ai/dsh-terminal': runtime,
    '@deepseek-ai/dsh-terminal-panel': runtime,
    '@deepseek-ai/dsh-tool-terminal': runtime,
    '@deepseek-ai/dsh-output-retention': runtime,
    '@deepseek-ai/dsh-error': runtime,
  };
  const rootManifests = new Map();
  for (const [name, root] of Object.entries(packageOverrides)) {
    if (!root) continue;
    const directory = join(root, 'node_modules', name);
    rootManifests.set(directory, {
      dir: await realpath(directory),
      manifest: JSON.parse(await readFile(join(directory, 'package.json'), 'utf8')),
    });
  }
  async function resolve(name) {
    if (resolved.has(name)) return;
    // Resolving while scanning also keeps recursive package graphs finite.
    resolved.set(name, null);
    const override = packageOverrides[name];
    if (override) {
      const directory = join(override, 'node_modules', name);
      const source = rootManifests.get(directory);
      resolved.set(name, { dir: source.dir, manifest: source.manifest });
    }
    let source = resolved.get(name);
    if (!source) for (const anchor of anchors) {
      source = await packageAt(
        createRequire(anchor), name,
        name.startsWith('@deepseek-ai/dsh-') ? manifest.dependencies[name] : undefined,
      );
      if (source) break;
    }
    if (!source) {
      // Follow the declaring package's own link, including pnpm's isolated
      // node_modules; bundle peer declarations are not normal dependencies.
      const provider = [...resolved.values()].find(candidate => candidate && (
        candidate.manifest.dependencies?.[name] || candidate.manifest.peerDependencies?.[name]
      ));
      if (provider) source = await packageAt(createRequire(join(provider.dir, 'package.json')), name);
    }
    if (!source) {
      // The terminal tools only use the abstract jobs service as an optional
      // capability. ACP supplies its local implementation, not a separately
      // published abstract package.
      const implementation = name === '@deepseek-ai/dsh-jobs'
        ? '@deepseek-ai/dsh-jobs-local' : name;
      if (implementation !== name) for (const anchor of anchors) {
        source = await packageAt(createRequire(anchor), implementation);
        if (source) break;
      }
    }
    if (!source) {
      const expected = name.startsWith('@deepseek-ai/dsh-') ? ` ${manifest.dependencies[name]}` : '';
      throw new Error(`ACP 安装缺少 Agent 终端依赖：${name}${expected}。`);
    }
    resolved.set(name, source);
    for (const dependency of [
      ...Object.keys(source.manifest.dependencies ?? {}),
      ...Object.keys(source.manifest.peerDependencies ?? {}),
    ]) await resolve(dependency);
  }
  for (const dependency of [
    ...Object.keys(manifest.dependencies),
    ...Object.keys(manifest.peerDependencies),
    ...Object.keys(packageOverrides),
  ]) await resolve(dependency);
  // The controller consumes the ACP provider's spawnTerminal interface; the
  // provider is never replaced by a package with a different release version.
  await mkdir(join(home, 'profiles/acp'), { recursive: true });
  for (const source of resolved.values()) {
    if (source) await linkTerminalPackage(home, source);
  }
  // The local provider owns PTYs; this interface only supplies its typed seam.
  return controller;
}

/** Project the controller's shell profile to the display name used by GPUI. */
export function terminalInfo(info) {
  return { ...info, shell: info.shell?.name ?? info.shell };
}

/** Normalize both roster entries and per-attachment state before JSON transport. */
export function terminalFrame(frame) {
  return frame.info ? { ...frame, info: terminalInfo(frame.info) } : frame;
}

/** Own one terminal attachment and its ordered, bounded Host command stream. */
export class AgentTerminalAttachment {
  constructor(controller, agent, terminalId, emit) {
    this.controller = controller;
    this.agent = agent;
    this.terminalId = terminalId;
    this.emit = emit;
    this.abort = new AbortController();
    this.input = new EventEmitter();
    this.commands = [];
    this.pending = new Map();
    this.sequence = 0;
    this.unacknowledgedBytes = 0;
    this.maxInputBytes = 64 * 1024;
    this.closed = new Error('终端连接已关闭。');
    this.failure = null;
    this.closing = false;
    this.started = false;
  }

  async command(command) {
    if (!this.started || this.abort.signal.aborted || this.failure) throw this.closed;
    if (!Number.isSafeInteger(command.sequence) || command.sequence !== this.sequence + 1) {
      throw new Error('终端输入序号不连续。');
    }
    if (!['write', 'resize'].includes(command.type)) throw new Error('终端操作无效。');
    const bytes = command.type === 'write' ? Buffer.byteLength(command.data, 'utf8') : 0;
    if (this.unacknowledgedBytes + bytes > this.maxInputBytes) throw new Error('终端输入尚未确认，请稍后重试。');
    this.sequence = command.sequence;
    this.unacknowledgedBytes += bytes;
    const reply = new Promise((resolve, reject) => this.pending.set(command.sequence, { resolve, reject, bytes }));
    this.commands.push(command);
    this.input.emit('command');
    try {
      return { sequence: command.sequence, ...await reply };
    } finally {
      this.pending.delete(command.sequence);
    }
  }

  async *inputs() {
    while (!this.abort.signal.aborted) {
      while (this.commands.length > 0) yield this.commands.shift();
      if (this.abort.signal.aborted) return;
      await once(this.input, 'command');
    }
  }

  async start() {
    if (this.started) throw new Error('终端已连接。');
    const environment = this.controller.environment(this.agent, this.abort.signal);
    this.maxInputBytes = environment.maxInputBytes;
    const invocation = {
      request: { namespace: 'terminal', method: 'attach', args: {} },
      service: 'terminalController',
      signal: this.abort.signal,
      peer: {
        id: 'kwork-terminal', ctx: this.agent.ctx, principal: { kind: 'owner' },
        interactions: 'answer', signal: this.abort.signal, dispose: async () => {},
      },
      uplink: () => this.inputs(),
    };
    // The public Remote invocation context is the supported in-process path to
    // BrowserTerminal.attach; no private controller fields are inspected.
    // The controller already declares its own service dependencies. Extending
    // the Agent context here would create an undeclared second service lookup;
    // only the public Remote invocation is added to its existing context.
    const attached = this.controller.ctx.extend({ invocation })
      .terminalController.attach(this.agent, this.terminalId, this.abort.signal);
    this.stream = attached[Symbol.asyncIterator] ? attached[Symbol.asyncIterator]() : attached;
    const first = await this.stream.next();
    if (first.done) throw new Error('终端连接已关闭。');
    this.publish(first.value);
    this.started = true;
    return { attached: true, frame: terminalFrame(first.value) };
  }

  async pump() {
    try {
      while (true) {
        const next = await this.stream.next();
        if (next.done) break;
        this.publish(next.value);
      }
      if (this.abort.signal.aborted) this.fail(this.closed);
      else this.fail(new Error('终端已退出。'));
      if (!this.abort.signal.aborted) {
        this.emit({ kind: 'agent_terminal_error', sessionId: this.agent.id, terminalId: this.terminalId,
          error: '终端连接已结束；可以重新连接。' });
      }
    } catch (error) {
      if (!this.abort.signal.aborted) {
        this.emit({ kind: 'agent_terminal_error', sessionId: this.agent.id, terminalId: this.terminalId,
          error: error instanceof Error ? error.message : String(error) });
      }
      this.fail(this.abort.signal.aborted ? this.closed : error);
    } finally {
      this.abort.abort();
      this.input.emit('command');
    }
  }

  publish(frame) {
    if (frame.type === 'ack') {
      const pending = this.pending.get(frame.sequence);
      if (pending) {
        this.unacknowledgedBytes -= pending.bytes;
        pending.resolve({ refusal: frame.refusal });
      }
      this.pending.delete(frame.sequence);
      return;
    }
    const kind = frame.type === 'state' ? 'agent_terminal_state' : 'agent_terminal_frame';
    this.emit({ kind, sessionId: this.agent.id, terminalId: this.terminalId, frame: terminalFrame(frame) });
  }

  fail(error) {
    this.failure ??= error;
    for (const pending of this.pending.values()) pending.reject(this.failure);
    this.pending.clear();
    this.unacknowledgedBytes = 0;
  }

  async close() {
    if (this.closing) return;
    this.closing = true;
    this.abort.abort();
    this.input.emit('command');
    this.fail(this.closed);
    await this.stream?.return?.();
  }
}
