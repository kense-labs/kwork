// Run from Bun's dev/build hooks as well as the macOS app bundle builder.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

if (process.platform === 'darwin' && !process.env.DSH_ACP_PATH) {
  const cwd = fileURLToPath(new URL('../services/macos-host/', import.meta.url));
  const install = spawnSync(process.execPath, ['install', '--frozen-lockfile', '--linker', 'hoisted', '--ignore-scripts'],
    { cwd, stdio: 'inherit' });
  if (install.error) throw install.error;
  process.exitCode = install.status ?? 1;
}
