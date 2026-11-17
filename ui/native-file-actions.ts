/** Trusted-frame transport for the upstream file action capability. */
import { BrowserFileActions, exportName } from './vendor/harness-images/files';
import type { FileActions, FileActionSource } from './vendor/harness-images/file-types';

export const invokeFileAction = <T = any>(command: string, args: object = {}): Promise<T> =>
  (window as any).__TAURI__.core.invoke(command, args);
export const openExternalFileLink = (url: string) => invokeFileAction('open_url', { url });
async function originalUrl(url: string): Promise<Blob> {
  const parsed = new URL(url);
  if (parsed.username || parsed.password) throw new Error('Unsupported file source');
  if (['http:', 'https:'].includes(parsed.protocol) && parsed.origin !== location.origin) {
    const result = await invokeFileAction('read_file_image', { url });
    return new Blob([Uint8Array.from(atob(result.base64), c => c.charCodeAt(0))], { type: result.mediaType });
  }
  if (!['http:', 'https:', 'blob:', 'data:'].includes(parsed.protocol)) throw new Error('Unsupported file source');
  const response = await fetch(url, { referrerPolicy: 'no-referrer', credentials: 'omit' });
  if (!response.ok) throw new Error('Original file unavailable');
  return response.blob();
}
const browser = new BrowserFileActions({
  async save({ url, name, mediaType, mode }) {
    const blob = await originalUrl(url);
    return invokeFileAction('save_file_bytes', { name: exportName(name, blob.type || mediaType || ''),
      data: [...new Uint8Array(await blob.arrayBuffer())], mode });
  },
  async readClipboardImage(url) {
    const blob = await originalUrl(url);
    return { bytes: new Uint8Array(await blob.arrayBuffer()), mediaType: blob.type };
  },
});
export const nativeFileActions: FileActions = {
  canSaveAs: true,
  save: browser.save.bind(browser),
  async copyImage(source: FileActionSource) {
    // WebKit AXPress has no transient browser activation. Its native fallback
    // reads the same original, validates/encodes it, and acknowledges the pasteboard.
    try { await browser.copyImage(source); }
    catch (error) {
      if (!document.documentElement.hasAttribute('data-native-mac-material')) throw error;
      const blob = source.read ? await source.read(new AbortController().signal) : await originalUrl(source.url);
      await invokeFileAction('copy_image_bytes', { data: [...new Uint8Array(await blob.arrayBuffer())] });
    }
  },
};
window.addEventListener('pagehide', () => browser.dispose(), { once: true });
