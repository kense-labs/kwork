/** Private transport for KWork's owned account-page child window. */
import { app, BrowserWindow, ipcMain } from 'electron'
import { once } from 'node:events'
import { createConnection } from 'node:net'
import { createInterface } from 'node:readline'
import { isAbsolute, join } from 'node:path'
import type { Writable } from 'node:stream'
import { DesktopPlatformView, PLATFORM_IPC } from './vendor/platform-view.ts'
import type { PlatformLocale } from './vendor/platform-ipc.ts'
import type { PlatformSession } from './vendor/types.ts'

function argument(name: string): string | undefined {
  return process.argv.find(value => value.startsWith(`${name}=`))?.slice(name.length + 1)
}
const directory = argument('--kwork-platform-data')
const version = argument('--kwork-client-version')
const pipePath = argument('--kwork-platform-pipe')
if (!directory || !isAbsolute(directory) || !version) process.exit(2)
if (process.platform === 'win32' && !pipePath?.startsWith('\\\\.\\pipe\\kwork-account-')) process.exit(2)
app.setPath('userData', directory)
process.env.DSH_CLIENT_VERSION = version
app.enableSandbox()
app.setName('KWork Account Platform')
app.on('window-all-closed', () => {})

type Request = { id: number; action: string; account?: PlatformSession; page?: 'usage' | 'top-up';
  locale?: PlatformLocale; width?: number; height?: number }
let output: Writable
function send(message: object): void { output.write(`${JSON.stringify(message)}\n`) }
let closing = false
let locale: PlatformLocale = 'zh_CN'
let owner: BrowserWindow | undefined
let current: DesktopPlatformView | undefined
let opening = false
let visitId: number | undefined
let documentAlive = false
let initialRedirectBlocked = false
const preload = join(__dirname, 'preload.cjs')

async function close(id?: number): Promise<void> {
  if (closing) return
  closing = true
  let ok = true
  try { await current?.dispose() } catch { ok = false }
  owner?.destroy()
  if (id !== undefined) send({ id, ok })
  output.write('', () => app.exit(ok ? 0 : 1))
}

async function run(): Promise<void> {
const pipe = pipePath ? createConnection(pipePath) : undefined
if (pipe) {
  await once(pipe, 'connect')
  pipe.on('error', () => { void close() })
}
output = pipe ?? process.stdout
await app.whenReady()
owner = new BrowserWindow({ width: 1, height: 1, show: false, frame: false, skipTaskbar: true,
  useContentSize: true, backgroundColor: '#ffffff',
  webPreferences: { sandbox: true, contextIsolation: true, nodeIntegration: false, devTools: false },
})
owner.setMenu(null)
const platform = current = new DesktopPlatformView(preload, () => locale,
  process.platform === 'win32' ? 'win32' : 'darwin', () => closing, (contents, origin) => {
  documentAlive = true
  contents.on('will-redirect', (event, url, _inPlace, isMainFrame) => {
    if (!opening || !isMainFrame) return
    const destination = new URL(url)
    if (destination.origin !== origin || destination.username || destination.password) {
      initialRedirectBlocked = true
      event.preventDefault()
    }
  })
  const failed = () => {
    documentAlive = false
    if (!closing && visitId !== undefined) send({ event: 'failed', id: visitId })
  }
  contents.once('preload-error', failed)
  contents.once('render-process-gone', failed)
  contents.once('destroyed', () => { documentAlive = false })
})
owner.on('resize', () => {
  if (!owner || owner.isDestroyed()) return
  const [width, height] = owner.getContentSize()
  platform.setBounds({ x: 0, y: 0, width, height })
})
owner.on('closed', () => { if (!closing) void close() })
ipcMain.on(PLATFORM_IPC.bootstrap, event => {
  try { event.returnValue = platform.bootstrap(event) }
  catch { event.returnValue = null }
})

async function handle(request: Request): Promise<void> {
  const { id, action } = request
  if (!Number.isSafeInteger(id) || id < 0) return
  if (action === 'close') { await close(id); return }
  if (closing) { send({ id, ok: false }); return }
  if (action === 'present') { owner?.showInactive(); send({ id, ok: true }); return }
  if (action === 'hide') { owner?.hide(); send({ id, ok: true }); return }
  if (action === 'locale' && (request.locale === 'zh_CN' || request.locale === 'en_US')) {
    locale = request.locale; platform.notifyLocaleChanged(); send({ id, ok: true }); return
  }
  if (action !== 'open' || opening || visitId !== undefined) { send({ id, ok: false }); return }
  const { account, page, width, height } = request
  if (!account || typeof account.token !== 'string' || !account.token
    || typeof account.origin !== 'string' || (page !== 'usage' && page !== 'top-up')
    || !Number.isFinite(width) || !Number.isFinite(height) || width! < 1 || height! < 1
    || width! > 100_000 || height! > 100_000) { send({ id, ok: false }); return }
  try {
    const origin = new URL(account.origin)
    if (origin.origin !== account.origin || origin.username || origin.password
      || !['http:', 'https:'].includes(origin.protocol)) throw new Error('Invalid origin')
    opening = true; visitId = id
    if (request.locale === 'zh_CN' || request.locale === 'en_US') locale = request.locale
    owner!.setContentSize(Math.round(width!), Math.round(height!))
    platform.setSession(account)
    await platform.open(owner!, page, { x: 0, y: 0, width: Math.round(width!), height: Math.round(height!) })
    if (!closing) send({ id, ok: documentAlive && !initialRedirectBlocked })
  } catch { if (!closing) send({ id, ok: false }) }
  finally { opening = false }
}

// The HWND is only for the trusted host's SetParent/SetWindowPos calls. No
// credentials are written to arguments, temporary files, or diagnostic output.
send({ event: 'ready', pid: process.pid, handle: owner.getNativeWindowHandle().toString('hex') })
const input = createInterface({ input: pipe ?? process.stdin, crlfDelay: Infinity })
input.on('line', line => {
  try { void handle(JSON.parse(line)).catch(() => { void close() }) }
  catch { void close() }
})
input.on('close', () => { void close() })
}
void run().catch(() => app.exit(2))
