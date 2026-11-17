/** Port the companion ACP transport extensions onto the version-matched Host. */
import { build } from 'esbuild';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { adaptPresetAcpEntry } from '../services/agent-preset-session.mjs';

const source = resolve(process.argv[2] ?? '');
if (!process.argv[2]) throw Error('Usage: build-agent-preset-acp.mjs <companion runtime>');
const original = join(source, 'packages/acp/acp/lib/index.js');
const folder = dirname(original);
const temp = await mkdtemp(join(tmpdir(), 'kwork-acp-preset-build-'));
const out = new URL('../services/vendor/dsh-agent-presets/', import.meta.url);
try {
  const session = join(temp, 'session.mjs');
  const url = path => JSON.stringify(pathToFileURL(path).href);
  await writeFile(session, `
import { AcpSession as Original } from ${url(join(folder, 'session.js'))};
import { mountAcpMcpServers } from ${url(join(folder, 'mcp.js'))};
import { installModelSelection } from '@deepseek-ai/dsh-agent';
import { presetAcpSession } from ${JSON.stringify(new URL('../services/agent-preset-session.mjs', import.meta.url).href)};
export const AcpSession = presetAcpSession({ AcpSession: Original, installModelSelection, mountAcpMcpServers });
`);
  const entry = adaptPresetAcpEntry(await readFile(original, 'utf8'), pathToFileURL(original).href, pathToFileURL(session).href);
  const result = await build({ stdin: { contents: entry, resolveDir: folder, sourcefile: 'kwork-acp-entry.mjs' },
    bundle: true, packages: 'external', platform: 'node', target: 'node24', format: 'esm',
    write: false, metafile: true, minifyWhitespace: true, legalComments: 'inline', tsconfigRaw: {},
    plugins: [{ name: 'owned-acp-files', setup(build) {
      build.onResolve({ filter: /^file:/ }, args => ({ path: fileURLToPath(args.path) }));
      // The source checkout's tsconfig aliases must never embed a second Host.
      build.onResolve({ filter: /^[^./]/ }, args => ({ path: args.path, external: true }));
    } }] });
  await mkdir(out, { recursive: true });
  await writeFile(new URL('acp.mjs', out), '// KWork ACP transport and preset adapter. MIT; see LICENSE and acp-source.json.\n' + result.outputFiles[0].text);
  const inputs = [];
  for (const path of Object.keys(result.metafile.inputs)) {
    if (path.endsWith('kwork-acp-entry.mjs') || resolve(path) === session) continue;
    if (!resolve(path).startsWith(folder + '/') && resolve(path) !== fileURLToPath(new URL('../services/agent-preset-session.mjs', import.meta.url))) {
      throw Error('Unexpected Host code in ACP adapter: ' + path);
    }
    const bytes = await readFile(path);
    inputs.push({ file: path.includes('/packages/acp/acp/') ? path.slice(path.indexOf('/packages/acp/acp/') + 1) : path,
      sha256: createHash('sha256').update(bytes).digest('hex') });
  }
  await writeFile(new URL('acp-source.json', out), JSON.stringify({ upstream: 'https://github.com/kense-labs/dsh-acp',
    purpose: 'ACP autonomous turn notifications, reserved/background session ownership, and pre-publication presets',
    targetHost: '0.2.2-rc.3', inputs }, null, 2) + '\n');
  console.log('Built KWork ACP adapter: ' + result.outputFiles[0].contents.length + ' bytes');
} finally { await rm(temp, { recursive: true, force: true }); }
