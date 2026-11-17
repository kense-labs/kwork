/** Resolve preset packages against one ACP installation, preserving service identities. */
import { readFile, readdir, realpath } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { join } from 'node:path';

async function readPackage(directory) {
  try {
    const manifest = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8'));
    return { dir: await realpath(directory), manifest };
  } catch (error) { if (error.code === 'ENOENT') return undefined; throw error; }
}

async function packageAt(anchor, name) {
  for (const parent of createRequire(anchor).resolve.paths(name) ?? []) {
    const found = await readPackage(join(parent, name));
    if (found?.manifest.name === name) return found;
  }
}

/** Missing entries are reported together so an incomplete profile is never activated. */
export async function resolvePresetPackages({ service, runtime, terminalRuntime, roots }) {
  const installed = new Map();
  const packageRoot = join(service, 'packages');
  for (const group of await readdir(packageRoot, { withFileTypes: true })) {
    if (!group.isDirectory() || group.name === 'node_modules') continue;
    for (const entry of await readdir(join(packageRoot, group.name), { withFileTypes: true })) {
      if ((!entry.isDirectory() && !entry.isSymbolicLink()) || entry.name === 'node_modules') continue;
      const found = await readPackage(join(packageRoot, group.name, entry.name));
      if (found) installed.set(found.manifest.name, found);
    }
  }
  const anchors = [join(service, 'apps/cli/package.json'), join(service, 'packages/bundle/base/package.json'),
    join(service, 'packages/api/gateway/package.json'), join(runtime, 'package.json')];
  const sources = new Map(), missing = new Map(), visited = new Set();
  // These packages already belong to the terminal controller's runtime. One
  // process must not import two copies of their services/classes.
  const terminalPackages = new Set(['@deepseek-ai/dsh-terminal', '@deepseek-ai/dsh-terminal-panel',
    '@deepseek-ai/dsh-tool-terminal', '@deepseek-ai/dsh-output-retention', '@deepseek-ai/dsh-error']);
  async function visit(name, provider, range, optional = false) {
    if (visited.has(name)) return;
    let found;
    if (terminalRuntime && terminalPackages.has(name)) found = await readPackage(join(terminalRuntime, 'node_modules', name));
    found ??= await readPackage(join(runtime, 'node_modules', name));
    // Shared Host packages take precedence over a plugin's private copies.
    found ??= installed.get(name);
    if (!found && provider) found = await packageAt(join(provider.dir, 'package.json'), name);
    if (!found) for (const anchor of anchors) { found = await packageAt(anchor, name); if (found) break; }
    if (!found && name === '@deepseek-ai/dsh-jobs') {
      found = installed.get('@deepseek-ai/dsh-jobs-local');
    }
    if (!found) {
      if (!optional) {
        const entry = missing.get(name) ?? { name, requestedBy: [], ranges: [] };
        if (provider) entry.requestedBy.push(provider.manifest.name);
        if (range) entry.ranges.push(range);
        missing.set(name, entry);
      }
      return;
    }
    visited.add(name); sources.set(name, found); missing.delete(name);
    for (const [dependency, version] of Object.entries(found.manifest.dependencies ?? {})) {
      await visit(dependency, found, version);
    }
    for (const [dependency, version] of Object.entries(found.manifest.peerDependencies ?? {})) {
      await visit(dependency, found, version, found.manifest.peerDependenciesMeta?.[dependency]?.optional === true);
    }
  }
  for (const root of roots) await visit(root);
  return { sources, missing: [...missing.values()] };
}
