import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import {
  buildMsixMapping,
  findWindowsSdkBinDir,
  handleSignHook,
  renderAppxManifest,
  toMsixVersion,
  writeTauriSignConfig,
} from './windows-packaging.mjs';

test('toMsixVersion maps SemVer monotonically and enforces Partner Center rules', () => {
  assert.equal(toMsixVersion('0.1.0'), '1.1.0.0');
  assert.equal(toMsixVersion('0.1.1'), '1.1.1.0');
  assert.equal(toMsixVersion('0.2.0-preview.1'), '1.2.0.0');
  assert.equal(toMsixVersion('1.0.0'), '2.0.0.0');
  assert.equal(toMsixVersion('0.1.0', '3.4.5.0'), '3.4.5.0');

  assert.throws(() => toMsixVersion('0.1.0', '0.1.0.0'), /rejects Major version 0/);
  assert.throws(() => toMsixVersion('0.1.0', '1.1.0.1'), /4th segment/);
});

test('renderAppxManifest produces valid Partner Center full-trust manifest XML', () => {
  const xml = renderAppxManifest({
    identityName: '12345Kense.KWork',
    publisher: 'CN=11111111-2222-3333-4444-555555555555',
    publisherDisplayName: 'Kense & Labs',
    displayName: 'KWork',
    msixVersion: '1.1.0.0',
  });
  assert.match(xml, /Name="12345Kense\.KWork"/);
  assert.match(xml, /Publisher="CN=11111111-2222-3333-4444-555555555555"/);
  assert.match(xml, /<PublisherDisplayName>Kense &amp; Labs<\/PublisherDisplayName>/);
  assert.match(xml, /EntryPoint="Windows\.FullTrustApplication"/);
  assert.match(xml, /<rescap:Capability Name="runFullTrust" \/>/);
  assert.equal((xml.match(/<Resource Language=/g) || []).length, 1);

  assert.throws(
    () => renderAppxManifest({ publisher: 'KenseLabs', msixVersion: '1.1.0.0' }),
    /starting with "CN="/,
  );
});

test('buildMsixMapping includes portable runtime layout and skips transient files', () => {
  const scratch = mkdtempSync(join(tmpdir(), 'kwork-msix-map-'));
  try {
    const iconsDir = join(scratch, 'icons');
    const releaseDir = join(scratch, 'release');
    mkdirSync(iconsDir, { recursive: true });
    mkdirSync(join(releaseDir, 'account-platform', 'resources', 'app'), { recursive: true });
    mkdirSync(join(releaseDir, 'harness-runtime', 'node_modules', '@deepseek-ai', 'dsh'), { recursive: true });
    mkdirSync(join(releaseDir, 'harness-runtime', '.incomplete-123'), { recursive: true });

    const manifestPath = join(scratch, 'AppxManifest.xml');
    writeFileSync(manifestPath, '<Package />');
    for (const icon of ['StoreLogo.png', 'Square44x44Logo.png', 'Square150x150Logo.png']) {
      writeFileSync(join(iconsDir, icon), 'png');
    }
    writeFileSync(join(releaseDir, 'KWork.exe'), 'MZ-main');
    writeFileSync(join(releaseDir, 'account-platform', 'electron.exe'), 'MZ-electron');
    writeFileSync(join(releaseDir, 'account-platform', 'resources', 'app', 'main.cjs'), 'main');
    writeFileSync(join(releaseDir, 'account-platform', 'resources', 'app', 'preload.cjs'), 'preload');
    writeFileSync(join(releaseDir, 'harness-runtime', 'node.exe'), 'MZ-node');
    writeFileSync(join(releaseDir, 'harness-runtime', 'node_modules', '@deepseek-ai', 'dsh', 'index.js'), 'dsh');
    writeFileSync(join(releaseDir, 'harness-runtime', '.DS_Store'), 'junk');
    writeFileSync(join(releaseDir, 'harness-runtime', '.incomplete-123', 'partial.js'), 'junk');

    const mapping = buildMsixMapping({ manifestPath, iconsDir, releaseDir });
    assert.ok(mapping.startsWith('[Files]\r\n'));
    assert.match(mapping, /"KWork\.exe"/);
    assert.match(mapping, /"Assets\\StoreLogo\.png"/);
    assert.match(mapping, /"account-platform\\electron\.exe"/);
    assert.match(mapping, /"harness-runtime\\node_modules\\@deepseek-ai\\dsh\\index\.js"/);
    assert.doesNotMatch(mapping, /\.DS_Store/);
    assert.doesNotMatch(mapping, /\.incomplete-/);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('handleSignHook collects and injects KWork.exe and NSIS temp uninstaller across two passes', () => {
  const scratch = mkdtempSync(join(tmpdir(), 'kwork-sign-hook-'));
  try {
    const unsignedDir = join(scratch, 'unsigned');
    const signedDir = join(scratch, 'signed');
    const mainExe = join(scratch, 'KWork.exe');
    const tempUninst = join(scratch, 'nsuA1B2.tmp');
    const pluginDll = join(scratch, 'System.dll');
    const outerSetup = join(scratch, 'KWork_0.1.0_x64-setup.exe');

    writeFileSync(mainExe, 'MZ-unsigned-main');
    writeFileSync(tempUninst, 'MZ-unsigned-uninst');
    writeFileSync(pluginDll, 'MZ-dll');
    writeFileSync(outerSetup, 'MZ-setup');

    assert.equal(
      handleSignHook({ targetPath: pluginDll, stage: 'collect', unsignedDir, signedDir }).action,
      'skip-dll',
    );
    assert.equal(
      handleSignHook({ targetPath: outerSetup, stage: 'collect', unsignedDir, signedDir }).action,
      'skip-outer-installer',
    );
    assert.equal(
      handleSignHook({ targetPath: mainExe, stage: 'collect', unsignedDir, signedDir }).action,
      'collected-main',
    );
    assert.equal(
      handleSignHook({ targetPath: tempUninst, stage: 'collect', unsignedDir, signedDir }).action,
      'collected-uninstaller',
    );

    assert.equal(readFileSync(join(unsignedDir, 'KWork.exe'), 'utf8'), 'MZ-unsigned-main');
    assert.equal(readFileSync(join(unsignedDir, 'uninstall.exe'), 'utf8'), 'MZ-unsigned-uninst');

    mkdirSync(signedDir, { recursive: true });
    writeFileSync(join(signedDir, 'KWork.exe'), 'MZ-SIGNED-MAIN');
    writeFileSync(join(signedDir, 'uninstall.exe'), 'MZ-SIGNED-UNINST');

    assert.equal(
      handleSignHook({ targetPath: mainExe, stage: 'inject', unsignedDir, signedDir }).action,
      'injected-main',
    );
    assert.equal(
      handleSignHook({ targetPath: tempUninst, stage: 'inject', unsignedDir, signedDir }).action,
      'injected-uninstaller',
    );

    assert.equal(readFileSync(mainExe, 'utf8'), 'MZ-SIGNED-MAIN');
    assert.equal(readFileSync(tempUninst, 'utf8'), 'MZ-SIGNED-UNINST');
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('writeTauriSignConfig normalizes Windows paths to forward slashes and findWindowsSdkBinDir picks newest SDK', () => {
  const scratch = mkdtempSync(join(tmpdir(), 'kwork-sdk-'));
  try {
    const outJson = join(scratch, 'tauri-sign.json');
    const cfg = writeTauriSignConfig(outJson, {
      nodeExecPath: 'C:\\Program Files\\nodejs\\node.exe',
      scriptPath: 'D:\\a\\kwork\\kwork\\scripts\\windows-packaging.mjs',
    });
    assert.ok(!cfg.bundle.windows.signCommand.cmd.includes('\\'));
    assert.ok(!cfg.bundle.windows.signCommand.args[0].includes('\\'));
    assert.equal(cfg.bundle.windows.signCommand.args[2], '%1');

    const oldSdk = join(scratch, 'Windows Kits', '10', 'bin', '10.0.19041.0', 'x64');
    const newSdk = join(scratch, 'Windows Kits', '10', 'bin', '10.0.26100.0', 'x64');
    mkdirSync(oldSdk, { recursive: true });
    mkdirSync(newSdk, { recursive: true });
    for (const dir of [oldSdk, newSdk]) {
      writeFileSync(join(dir, 'makeappx.exe'), 'MZ');
      writeFileSync(join(dir, 'signtool.exe'), 'MZ');
    }
    assert.equal(findWindowsSdkBinDir(scratch), newSdk);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});
