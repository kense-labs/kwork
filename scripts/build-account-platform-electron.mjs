import { build } from 'esbuild';
import { access, cp, mkdir, rm, writeFile } from 'node:fs/promises';
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
if (process.argv.includes('--windows-bundle')) {
  const runtime = join(source, 'node_modules/electron/dist');
  const bundle = join(source, 'bundle');
  await access(join(runtime, 'electron.exe'));
  await rm(bundle, { recursive: true, force: true });
  await cp(runtime, bundle, { recursive: true });
  await rm(join(bundle, 'resources/default_app.asar'), { force: true });
  await cp(outdir, join(bundle, 'resources/app'), { recursive: true });
  console.log(`Windows account renderer ready: ${bundle}`);
}
