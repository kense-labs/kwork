import { build } from 'esbuild';
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(new URL('../package.json', import.meta.url)));
const source = join(root, 'services/account-platform-electron');
const outdir = join(source, 'dist');
await mkdir(outdir, { recursive: true });
await build({
  entryPoints: { main: join(source, 'main.ts'), preload: join(source, 'vendor/preload-platform-account.ts') },
  outdir, outExtension: { '.js': '.cjs' }, bundle: true, platform: 'node', format: 'cjs',
  target: 'node24', external: ['electron'], logLevel: 'warning',
});
await writeFile(join(outdir, 'package.json'), JSON.stringify({ name: 'kwork-account-platform', version: '0.1.0', main: 'main.cjs', private: true }, null, 2) + '\n');
console.log(`Account Platform helper ready: ${outdir}`);
