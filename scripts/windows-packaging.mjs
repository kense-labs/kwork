import { execFileSync } from 'node:child_process';
import {
  copyFileSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync,
  readSync, readdirSync, rmSync, closeSync, writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptFile = fileURLToPath(import.meta.url);
const repoRoot = dirname(dirname(scriptFile));

export function toMsixVersion(appVersion, overrideVersion = '') {
  const explicit = overrideVersion.trim();
  if (explicit) {
    const match = /^(\d+)\.(\d+)\.(\d+)\.(\d+)$/.exec(explicit);
    if (!match) {
      throw new Error(`Invalid MS_STORE_VERSION "${explicit}": expected 4-part version Major.Minor.Build.0.`);
    }
    const parts = match.slice(1).map(Number);
    if (parts.some((n) => !Number.isInteger(n) || n < 0 || n > 65535)) {
      throw new Error(`Invalid MS_STORE_VERSION "${explicit}": each segment must be in 0..65535.`);
    }
    if (parts[0] < 1) {
      throw new Error(`Invalid MS_STORE_VERSION "${explicit}": Microsoft Partner Center rejects Major version 0.`);
    }
    if (parts[3] !== 0) {
      throw new Error(`Invalid MS_STORE_VERSION "${explicit}": Microsoft Partner Center reserves the 4th segment (must be 0).`);
    }
    return explicit;
  }

  const clean = String(appVersion).trim().split(/[-+]/)[0];
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(clean);
  if (!match) {
    throw new Error(`Unsupported SemVer "${appVersion}" for MSIX packaging.`);
  }
  const [major, minor, patch] = match.slice(1).map(Number);
  if (major > 65534 || minor > 65535 || patch > 65535) {
    throw new Error(`Version "${appVersion}" exceeds MSIX 16-bit version segment limits.`);
  }
  // Partner Center forbids Major == 0 and requires monotonic versions across
  // submissions; shifting Major by +1 maps 0.1.0 -> 1.1.0.0 and 1.0.0 -> 2.0.0.0
  // without collisions or ordering inversions.
  return `${major + 1}.${minor}.${patch}.0`;
}

export function escapeXml(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;');
}

export function renderAppxManifest({
  identityName = 'KenseLabs.KWork',
  publisher = 'CN=KenseLabs',
  publisherDisplayName = 'Kense Labs',
  displayName = 'KWork',
  description = 'Desktop client for DeepSeek Harness ACP',
  msixVersion,
  executable = 'KWork.exe',
  arch = 'x64',
  language = 'zh-CN',
}) {
  if (!/^[A-Za-z0-9.-]{3,50}$/.test(identityName)) {
    throw new Error(`Invalid MSIX Identity Name "${identityName}": expected 3-50 ASCII letters, digits, dots, or hyphens.`);
  }
  if (!publisher.startsWith('CN=')) {
    throw new Error(`Invalid MSIX Publisher "${publisher}": expected a Distinguished Name starting with "CN=".`);
  }
  toMsixVersion('0.1.0', msixVersion);

  return [
    '<?xml version="1.0" encoding="utf-8"?>',
    '<Package',
    '  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"',
    '  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"',
    '  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"',
    '  IgnorableNamespaces="uap rescap">',
    '  <Identity',
    `    Name="${escapeXml(identityName)}"`,
    `    Publisher="${escapeXml(publisher)}"`,
    `    Version="${escapeXml(msixVersion)}"`,
    `    ProcessorArchitecture="${escapeXml(arch)}" />`,
    '  <Properties>',
    `    <DisplayName>${escapeXml(displayName)}</DisplayName>`,
    `    <PublisherDisplayName>${escapeXml(publisherDisplayName)}</PublisherDisplayName>`,
    '    <Logo>Assets\\StoreLogo.png</Logo>',
    '  </Properties>',
    '  <Dependencies>',
    '    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0" />',
    '  </Dependencies>',
    '  <Resources>',
    `    <Resource Language="${escapeXml(language)}" />`,
    '  </Resources>',
    '  <Applications>',
    `    <Application Id="KWork" Executable="${escapeXml(executable)}" EntryPoint="Windows.FullTrustApplication">`,
    '      <uap:VisualElements',
    `        DisplayName="${escapeXml(displayName)}"`,
    `        Description="${escapeXml(description)}"`,
    '        BackgroundColor="transparent"',
    '        Square150x150Logo="Assets\\Square150x150Logo.png"',
    '        Square44x44Logo="Assets\\Square44x44Logo.png" />',
    '    </Application>',
    '  </Applications>',
    '  <Capabilities>',
    '    <rescap:Capability Name="runFullTrust" />',
    '  </Capabilities>',
    '</Package>',
    '',
  ].join('\r\n');
}

function shouldSkipBundledEntry(name) {
  return (
    name === '.DS_Store' ||
    name === '.archive' ||
    name.startsWith('.incomplete-') ||
    name.startsWith('.install-')
  );
}

function collectDirectoryMappings(sourceRoot, packagePrefix, mappings) {
  const entries = readdirSync(sourceRoot, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name));
  for (const entry of entries) {
    if (shouldSkipBundledEntry(entry.name)) continue;
    const sourcePath = join(sourceRoot, entry.name);
    const packagePath = `${packagePrefix}\\${entry.name}`;
    if (entry.isDirectory()) {
      collectDirectoryMappings(sourcePath, packagePath, mappings);
    } else if (entry.isFile()) {
      mappings.push([sourcePath, packagePath]);
    }
  }
}

export function buildMsixMapping({
  manifestPath,
  iconsDir,
  releaseDir,
  mainBinaryName = 'KWork.exe',
}) {
  const required = [
    [manifestPath, 'AppxManifest.xml'],
    [join(iconsDir, 'StoreLogo.png'), 'Assets\\StoreLogo.png'],
    [join(iconsDir, 'Square44x44Logo.png'), 'Assets\\Square44x44Logo.png'],
    [join(iconsDir, 'Square150x150Logo.png'), 'Assets\\Square150x150Logo.png'],
    [join(releaseDir, mainBinaryName), mainBinaryName],
  ];
  for (const [file] of required) {
    if (!existsSync(file)) {
      throw new Error(`Missing required MSIX input file: ${file}`);
    }
  }
  const requiredRuntimeFiles = [
    join(releaseDir, 'account-platform', 'electron.exe'),
    join(releaseDir, 'account-platform', 'resources', 'app', 'main.cjs'),
    join(releaseDir, 'account-platform', 'resources', 'app', 'preload.cjs'),
    join(releaseDir, 'harness-runtime', 'node.exe'),
  ];
  for (const file of requiredRuntimeFiles) {
    if (!existsSync(file)) {
      throw new Error(`Missing required Windows runtime file for MSIX: ${file}`);
    }
  }

  const mappings = [...required];
  collectDirectoryMappings(join(releaseDir, 'account-platform'), 'account-platform', mappings);
  collectDirectoryMappings(join(releaseDir, 'harness-runtime'), 'harness-runtime', mappings);

  const lines = ['[Files]'];
  for (const [sourcePath, packagePath] of mappings) {
    lines.push(`"${sourcePath}" "${packagePath}"`);
  }
  lines.push('');
  return lines.join('\r\n');
}

function parseSdkVersion(name) {
  const parts = name.split('.').map((part) => Number.parseInt(part, 10));
  return parts.every(Number.isFinite) ? parts : [0];
}

function compareVersionArrays(a, b) {
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i++) {
    const diff = (b[i] ?? 0) - (a[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

export function findWindowsSdkBinDir(
  programFilesX86 = process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)',
) {
  const kitsBin = join(programFilesX86, 'Windows Kits', '10', 'bin');
  if (!existsSync(kitsBin)) {
    throw new Error(`Windows SDK bin directory not found at ${kitsBin}`);
  }
  const candidates = readdirSync(kitsBin, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && /^10\./.test(entry.name))
    .sort((a, b) => compareVersionArrays(parseSdkVersion(a.name), parseSdkVersion(b.name)));
  for (const entry of candidates) {
    const x64Dir = join(kitsBin, entry.name, 'x64');
    if (existsSync(join(x64Dir, 'makeappx.exe')) && existsSync(join(x64Dir, 'signtool.exe'))) {
      return x64Dir;
    }
  }
  throw new Error(`Could not locate makeappx.exe and signtool.exe under ${kitsBin}`);
}

function assertPortableExecutable(filePath) {
  const fd = openSync(filePath, 'r');
  try {
    const header = Buffer.alloc(2);
    const bytesRead = readSync(fd, header, 0, 2, 0);
    if (bytesRead < 2 || header[0] !== 0x4d || header[1] !== 0x5a) {
      throw new Error(`Expected a Windows PE binary (MZ header), got: ${filePath}`);
    }
  } finally {
    closeSync(fd);
  }
}

export function handleSignHook({
  targetPath,
  stage = process.env.SIGNPATH_STAGE || '',
  unsignedDir = process.env.SIGNPATH_INNER_UNSIGNED_DIR || join(repoRoot, 'target', 'signpath-inner-unsigned'),
  signedDir = process.env.SIGNPATH_INNER_SIGNED_DIR || join(repoRoot, 'target', 'signpath-inner-signed'),
  mainBinaryName = process.env.SIGNPATH_MAIN_BINARY_NAME || 'KWork.exe',
}) {
  if (stage !== 'collect' && stage !== 'inject') {
    throw new Error(`Invalid SIGNPATH_STAGE "${stage}": expected "collect" or "inject".`);
  }
  if (!targetPath || !existsSync(targetPath)) {
    throw new Error(`Sign hook target does not exist: ${targetPath}`);
  }

  const name = basename(targetPath);
  const lowerName = name.toLowerCase();
  const lowerExt = extname(lowerName);

  // SignPath Foundation policies enforce ProductName == KWork on submitted PE
  // files; third-party NSIS plugin DLLs are left untouched.
  if (lowerExt === '.dll') {
    return { action: 'skip-dll', targetPath };
  }

  // The outer NSIS setup executable is submitted in Stage 2 after `tauri bundle`
  // finishes packing the signed inner executables.
  if (lowerName.endsWith('-setup.exe') || lowerName === 'nsis-output.exe') {
    return { action: 'skip-outer-installer', targetPath };
  }

  const isMain = lowerName === mainBinaryName.toLowerCase();
  const artifactFileName = isMain ? mainBinaryName : 'uninstall.exe';
  assertPortableExecutable(targetPath);

  if (stage === 'collect') {
    mkdirSync(unsignedDir, { recursive: true });
    const destination = join(unsignedDir, artifactFileName);
    copyFileSync(targetPath, destination);
    return {
      action: isMain ? 'collected-main' : 'collected-uninstaller',
      targetPath,
      destination,
    };
  }

  const signedSource = join(signedDir, artifactFileName);
  if (!existsSync(signedSource)) {
    throw new Error(`Missing SignPath-signed inner binary: ${signedSource}`);
  }
  assertPortableExecutable(signedSource);
  copyFileSync(signedSource, targetPath);
  return {
    action: isMain ? 'injected-main' : 'injected-uninstaller',
    targetPath,
    source: signedSource,
  };
}

function toForwardSlashAbsolutePath(inputPath) {
  const raw = String(inputPath);
  if (/^[A-Za-z]:[\\/]/.test(raw)) {
    return raw.replaceAll('\\', '/');
  }
  return resolve(raw).replaceAll('\\', '/');
}

export function writeTauriSignConfig(
  outputJsonPath,
  { nodeExecPath = process.execPath, scriptPath = scriptFile } = {},
) {
  // Forward slashes avoid backslash-escaping ambiguities when Tauri formats
  // the command into NSIS's `!uninstfinalize` directive.
  const config = {
    bundle: {
      windows: {
        signCommand: {
          cmd: toForwardSlashAbsolutePath(nodeExecPath),
          args: [toForwardSlashAbsolutePath(scriptPath), 'sign-hook', '%1'],
        },
      },
    },
  };
  mkdirSync(dirname(resolve(outputJsonPath)), { recursive: true });
  writeFileSync(outputJsonPath, `${JSON.stringify(config, null, 2)}\n`, 'utf8');
  return config;
}

function runMsixSmokeTest({ signtoolPath, msixPath, publisher, identityName }) {
  const scratch = mkdtempSync(join(process.env.RUNNER_TEMP || tmpdir(), 'kwork-msix-smoke-'));
  const smokeMsix = join(scratch, 'KWork-smoke.msix');
  const certFile = join(scratch, 'KWork-smoke.cer');
  copyFileSync(msixPath, smokeMsix);

  const psScript = `
$ErrorActionPreference = 'Stop'
$cert = $null
$trusted = $null
try {
  $cert = New-SelfSignedCertificate -Type Custom \`
    -Subject $env:KWORK_SMOKE_PUBLISHER \`
    -KeyUsage DigitalSignature \`
    -FriendlyName 'KWork MSIX Smoke Test' \`
    -CertStoreLocation 'Cert:\\LocalMachine\\My' \`
    -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
  Export-Certificate -Cert $cert -FilePath $env:KWORK_SMOKE_CER | Out-Null
  $trusted = Import-Certificate -FilePath $env:KWORK_SMOKE_CER -CertStoreLocation 'Cert:\\LocalMachine\\TrustedPeople'
  & $env:KWORK_SMOKE_SIGNTOOL sign /fd SHA256 /sha1 $cert.Thumbprint $env:KWORK_SMOKE_MSIX
  if ($LASTEXITCODE -ne 0) { throw "signtool exited with code $LASTEXITCODE" }
  Add-AppxPackage -Path $env:KWORK_SMOKE_MSIX -ErrorAction Stop
  $pkg = Get-AppxPackage -Name $env:KWORK_SMOKE_IDENTITY
  if (-not $pkg) { throw "MSIX package $($env:KWORK_SMOKE_IDENTITY) was not registered" }
  foreach ($rel in @('KWork.exe', 'account-platform\\electron.exe', 'harness-runtime\\node.exe')) {
    $full = Join-Path $pkg.InstallLocation $rel
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) {
      throw "Missing installed MSIX payload file: $full"
    }
  }
  Write-Host "Verified MSIX installation at $($pkg.InstallLocation)"
} finally {
  $installed = Get-AppxPackage -Name $env:KWORK_SMOKE_IDENTITY -ErrorAction SilentlyContinue
  if ($installed) { Remove-AppxPackage -Package $installed.PackageFullName -ErrorAction SilentlyContinue }
  if ($trusted) { Remove-Item -LiteralPath "Cert:\\LocalMachine\\TrustedPeople\\$($trusted.Thumbprint)" -Force -ErrorAction SilentlyContinue }
  if ($cert) { Remove-Item -LiteralPath "Cert:\\LocalMachine\\My\\$($cert.Thumbprint)" -Force -ErrorAction SilentlyContinue }
}
`;

  try {
    execFileSync('pwsh', ['-NoProfile', '-NonInteractive', '-Command', psScript], {
      stdio: 'inherit',
      env: {
        ...process.env,
        KWORK_SMOKE_PUBLISHER: publisher,
        KWORK_SMOKE_IDENTITY: identityName,
        KWORK_SMOKE_SIGNTOOL: signtoolPath,
        KWORK_SMOKE_MSIX: smokeMsix,
        KWORK_SMOKE_CER: certFile,
      },
    });
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

export function buildMsix({
  root = repoRoot,
  smokeTest = false,
  env = process.env,
} = {}) {
  const tauriConf = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const productName = tauriConf.productName || 'KWork';
  const appVersion = tauriConf.version || '0.1.0';
  const mainBinaryName = `${tauriConf.mainBinaryName || 'KWork'}.exe`;

  const identityName = (env.MS_STORE_IDENTITY_NAME || '').trim() || 'KenseLabs.KWork';
  const publisher = (env.MS_STORE_PUBLISHER || '').trim() || 'CN=KenseLabs';
  const publisherDisplayName = (env.MS_STORE_PUBLISHER_DISPLAY_NAME || '').trim() || 'Kense Labs';
  const msixVersion = toMsixVersion(appVersion, env.MS_STORE_VERSION || '');

  const workDir = join(root, 'target', 'msix-build');
  const outDir = join(root, 'target', 'release', 'bundle', 'msix');
  rmSync(workDir, { recursive: true, force: true });
  mkdirSync(workDir, { recursive: true });
  mkdirSync(outDir, { recursive: true });

  const manifestPath = join(workDir, 'AppxManifest.xml');
  const mappingPath = join(workDir, 'mapping.txt');
  const manifestXml = renderAppxManifest({
    identityName,
    publisher,
    publisherDisplayName,
    displayName: productName,
    msixVersion,
    executable: mainBinaryName,
  });
  // Write UTF-8 without BOM so Win32 INI / XML parsers in makeappx.exe read cleanly.
  writeFileSync(manifestPath, Buffer.from(manifestXml, 'utf8'));

  const mappingContent = buildMsixMapping({
    manifestPath,
    iconsDir: join(root, 'src-tauri', 'icons'),
    releaseDir: join(root, 'target', 'release'),
    mainBinaryName,
  });
  writeFileSync(mappingPath, Buffer.from(mappingContent, 'utf8'));

  const sdkBin = findWindowsSdkBinDir(env['ProgramFiles(x86)']);
  const makeappxPath = join(sdkBin, 'makeappx.exe');
  const signtoolPath = join(sdkBin, 'signtool.exe');
  const outputMsix = join(outDir, `${productName}_${appVersion}_x64.msix`);

  execFileSync(makeappxPath, ['pack', '/f', mappingPath, '/p', outputMsix, '/o'], {
    stdio: 'inherit',
  });
  console.log(`MSIX package ready (Partner Center upload, Version=${msixVersion}, Identity=${identityName}): ${outputMsix}`);

  if (smokeTest) {
    runMsixSmokeTest({
      signtoolPath,
      msixPath: outputMsix,
      publisher,
      identityName,
    });
  }

  return { outputMsix, msixVersion, identityName, publisher };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(scriptFile)) {
  const [command, ...rest] = process.argv.slice(2);
  if (command === 'sign-hook') {
    const result = handleSignHook({ targetPath: rest[0] });
    console.log(`[signpath-hook] ${result.action}: ${result.targetPath}`);
  } else if (command === 'write-tauri-sign-config') {
    if (!rest[0]) throw new Error('Usage: windows-packaging.mjs write-tauri-sign-config <output.json>');
    writeTauriSignConfig(rest[0]);
    console.log(`Wrote Tauri SignPath config to ${rest[0]}`);
  } else if (command === 'build-msix') {
    buildMsix({ smokeTest: rest.includes('--smoke-test') });
  } else {
    throw new Error(`Unknown command "${command ?? ''}". Expected: sign-hook | write-tauri-sign-config | build-msix`);
  }
}
