/** Bind the official preset registry during ACP's unpublished Agent setup. */
export function presetAcpSession({ AcpSession, installModelSelection, mountAcpMcpServers }) {
  const registryOf = ctx => {
    const registry = ctx.get('agentPresets');
    if (!registry) throw new Error('Agent 预设服务尚未就绪。');
    return registry;
  };
  const resolve = async (registry, requested) => {
    if (requested !== undefined && (typeof requested !== 'string' || !requested.trim())) {
      throw new Error('Agent 预设标识无效。');
    }
    const preset = await registry.resolve(requested);
    if (preset.broken !== undefined) throw new Error(preset.broken);
    return preset.id;
  };
  const compose = async (registry, id, agentCtx, options) => {
    options.signal?.throwIfAborted();
    installModelSelection(agentCtx);
    await registry.mount(agentCtx, id);
    options.signal?.throwIfAborted();
    await mountAcpMcpServers(agentCtx, options.mcpServers, options.cwd);
    options.signal?.throwIfAborted();
  };
  return class PresetAcpSession extends AcpSession {
    static async create(ctx, options) {
      const registry = registryOf(ctx);
      // Resolve once, before writing the immutable creation header. A concurrent
      // default change must not make the recorded and mounted presets differ.
      const id = await resolve(registry, options.agentPreset);
      options.signal?.throwIfAborted();
      const handle = await ctx.agents.create({
        sessionId: options.sessionId,
        meta: { cwd: options.cwd, agentPreset: id },
        agentOptions: options.agentOptions,
        signal: options.signal,
        setup: agentCtx => compose(registry, id, agentCtx, options),
      });
      return new this(ctx, handle, options.notify);
    }

    static async resume(ctx, options) {
      const registry = registryOf(ctx);
      const handle = await ctx.agents.resume({
        resumeSessionId: options.sessionId,
        agentOptions: options.agentOptions,
        signal: options.signal,
        setup: async (agentCtx, agent) => {
          // The writer has now restored the committed projection, including
          // blank-session switches. Reading only the creation header is wrong.
          const id = await resolve(registry, registry.presetForSession(agent.session) ?? undefined);
          await compose(registry, id, agentCtx, options);
        },
      });
      return new this(ctx, handle, options.notify);
    }
  };
}

/**
 * Adapt only the ACP composition entry and its explicit Host preset argument.
 * The original module supplies every protocol/session lifecycle implementation.
 * Reject an unfamiliar source shape instead of silently losing a seam.
 */
export function adaptPresetAcpEntry(source, originalUrl, sessionUrl) {
  const replaceOnce = (pattern, replacement) => {
    const matches = [...source.matchAll(pattern)];
    if (matches.length !== 1) throw new Error('当前 ACP 会话入口与 Agent 预设适配器不兼容。');
    source = source.replace(pattern, replacement);
  };
  replaceOnce(/import \{ AcpSession \} from ["']\.\/session\.js["'];/g,
    `import { AcpSession } from ${JSON.stringify(sessionUrl)};`);
  replaceOnce(/sessionId, cwd: params\.cwd, mcpServers: params\.mcpServers,/g,
    "sessionId, cwd: params.cwd, mcpServers: params.mcpServers, agentPreset: params.agentPreset ?? params._meta?.['kwork.agentPreset'],");
  replaceOnce(/async create\(sessionId, cwd\) \{/g, 'supportsAgentPresets: true, async create(sessionId, cwd, agentPreset) {');
  replaceOnce(/createSession\(sessionId, \{ cwd, mcpServers: \[\] \}, lifetime\.signal, true\)/g,
    'createSession(sessionId, { cwd, mcpServers: [], agentPreset }, lifetime.signal, true)');
  // Preserve imports of the original content, model, MCP and error classes.
  // Bare packages still resolve through the profile's official package table.
  return source.replace(/(\bfrom\s+)(["'])(\.\.?\/[^"']+)\2/g,
    (_match, prefix, _quote, relative) => prefix + JSON.stringify(new URL(relative, originalUrl).href));
}
