/** Ordered tool-result content and image adapters for the official inspector. */
import { useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ImageGallery, type ImageLoader } from './vendor/harness-images/MessageImage';
import { FileTransfers } from './vendor/harness-images/transfers';
import { BrowserFileActions } from './vendor/harness-images/files';
import { createContextMenuBindings } from './vendor/harness-images/trigger';
import { resolveDragFileName } from './vendor/harness-images/file-names';
import { zh as fileLabels } from './vendor/harness-images/file-locales';
import { Menu } from './vendor/harness-primitives/Menu';
import { Toast } from './vendor/harness-primitives/Toast';
import { IconCopyOutlineRegular, IconDownloadOutlineRegular, IconSaveOutlineRegular } from './vendor/harness-primitives/icons';
import { JsonTree, type JsonTreeProps, type JsonTreeLabels } from './vendor/harness-primitives/JsonTree';

type Attachment = { attachmentId: string; mediaType: string; bytes: number; width: number; height: number; name?: string };
type SourceBlock = { type: string; content: string; attachment?: Attachment };
// Upstream layout.ts sourceBlock, retaining unknown blocks as escaped JSON.
export function sourceBlock(value: unknown): SourceBlock {
  const stringify = (value: unknown) => JSON.stringify(value, null, 2) || String(value);
  if (typeof value !== 'object' || value === null) return { type: 'unknown', content: stringify(value) };
  const block = value as Record<string, any>;
  const type = typeof block.type === 'string' ? block.type : 'unknown';
  if (typeof block.text === 'string') return { type: type === 'reasoning' ? 'thinking' : type, content: block.text };
  if ((type === 'image' || type === 'file') && typeof block.attachment === 'object' && block.attachment !== null
    && typeof block.attachment.attachmentId === 'string') {
    return { type, content: stringify(value), ...(type === 'image' ? { attachment: block.attachment } : {}) };
  }
  return { type, content: stringify(value) };
}
const images = new Map<string, { url?: string; pending: Promise<string> }>();
const loadImage: ImageLoader = Object.assign((attachment: Attachment) => {
  const key = `${attachment.attachmentId}:${attachment.mediaType}`;
  const cached = images.get(key);
  if (cached) return cached.pending;
  const entry = { url: undefined as string | undefined, pending: Promise.resolve('') };
  entry.pending = (window as any).__TAURI__.core.invoke('read_attachment', {
    attachmentId: attachment.attachmentId, mediaType: attachment.mediaType,
  }).then((reply: any) => {
    if (typeof reply?.dataUrl !== 'string' || !reply.dataUrl.startsWith('data:image/')) throw new Error('Image unavailable');
    entry.url = reply.dataUrl;
    return reply.dataUrl;
  }).catch((error: unknown) => { images.delete(key); throw error; });
  images.set(key, entry);
  if (images.size > 32) {
    const oldest = images.keys().next().value!;
    images.delete(oldest);
  }
  return entry.pending;
}, { peek: (attachment: Attachment) => images.get(`${attachment.attachmentId}:${attachment.mediaType}`)?.url });
const labels = { image: '图片', open: '查看原图', openNamed: (label: string) => `${label}，点击查看原图`,
  loading: '图片加载中…', loadFailed: '图片加载失败，点击重试', lightbox: { dialog: '原图预览', close: '关闭原图预览' } };
const copyActions = new BrowserFileActions(undefined);
let noticeRoot: ReturnType<typeof createRoot> | undefined;
let noticeId = 0;
function report(text: string, success = false, path?: string) {
  if (!noticeRoot) { const holder = document.createElement('div'); document.body.append(holder); noticeRoot = createRoot(holder); }
  const id = ++noticeId;
  const reveal = path ? [{ label: fileLabels['menu.finder'], onClick: () => {
    void (window as any).__TAURI__.core.invoke('open_path', { path, reveal: true }).catch(() => report(fileLabels['path.revealError']));
  } }] : undefined;
  noticeRoot.render(<Toast key={id} text={text} tone={success ? 'success' : undefined} actions={reveal}
    onDone={() => { if (id === noticeId) noticeRoot?.render(null); }} />);
}
// Transfer ownership survives record changes and the temporary drop overlay.
const transfers = new FileTransfers(undefined, () => report('拖放失败，请重试'));
const fileDrag = { prepare: transfers.prepare.bind(transfers), reportFailure: () => report('拖放失败，请重试') };
export const trajectoryFileDrag = fileDrag;
export const readTrajectoryImageDrag = (transfer: DataTransfer) => transfers.read(transfer);
window.addEventListener('pagehide', () => { transfers.dispose(); copyActions.dispose(); }, { once: true });

type ImageTarget = { kind: 'image'; value: { url: string; name?: string; mediaType?: string } };
function ResultImage({ attachment }: { attachment: Attachment }) {
  const [menu, setMenu] = useState<{ target: ImageTarget; point: { x: number; y: number }; opener: HTMLElement | null }>();
  const opener = useRef<HTMLElement | null>(null);
  const bindings = useMemo(() => createContextMenuBindings<ImageTarget>((target, point) => {
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setMenu({ target, point, opener: opener.current });
  }, () => true), []);
  const select = (id: string) => {
    if (!menu) return;
    const source = menu.target.value;
    setMenu(undefined);
    const operation: Promise<{status:string;path?:string}> = id === 'copy'
      ? copyActions.copyImage(source).catch(error => {
        if (!document.documentElement.hasAttribute('data-native-mac-material')) throw error;
        // WKWebView can deny clipboard writes from native accessibility actions.
        return (window as any).__TAURI__.core.invoke('copy_image_attachment', {
          attachmentId: attachment.attachmentId.startsWith('sha256:') ? attachment.attachmentId : `sha256:${attachment.attachmentId}`,
          bytes: attachment.bytes,
        });
      }).then(() => ({ status: 'copied' }))
      : (window as any).__TAURI__.core.invoke('save_image_attachment', {
        attachmentId: attachment.attachmentId.startsWith('sha256:') ? attachment.attachmentId : `sha256:${attachment.attachmentId}`,
        name: resolveDragFileName({ name: source.name ?? attachment.name ?? 'image', mediaType: attachment.mediaType }),
        bytes: attachment.bytes, mode: id === 'saveAs' ? 'saveAs' : 'download',
      });
    void operation.then(result => {
      if (result.status !== 'cancelled') report(id === 'copy' ? fileLabels['menu.copied'] : id === 'saveAs' ? fileLabels['menu.saved'] : fileLabels['menu.downloaded'], true, result.path);
    }).catch(() => report(fileLabels[id === 'copy' ? 'menu.copyFailed' : 'menu.saveFailed']));
  };
  return <>
    <ImageGallery images={[{ attachment }]} align="start" load={loadImage} labels={labels} fileDrag={fileDrag} getTriggerProps={bindings.getTriggerProps} />
    <Menu open={!!menu} anchor={null} portal autoFocus contextual anchorPoint={menu?.point}
      returnFocus={() => opener.current} onClose={() => setMenu(undefined)} onSelect={select}
      items={[
        { id: 'copy', label: fileLabels['menu.copyImage'], icon: <IconCopyOutlineRegular /> },
        { id: 'download', label: fileLabels['menu.download'], icon: <IconDownloadOutlineRegular /> },
        { id: 'saveAs', label: fileLabels['menu.saveAs'], icon: <IconSaveOutlineRegular /> },
      ]} />
  </>;
}
export function ToolOutput({ value, blocks: raw = [], error, preview, labels: jsonLabels, stringWrapping }:
  { value: string; blocks?: unknown[]; error: boolean; preview: boolean; labels: JsonTreeLabels; stringWrapping: JsonTreeProps['stringWrapping'] }) {
  const blocks = raw.map(sourceBlock);
  let parsed: object | undefined;
  if (!error) { try { const data = JSON.parse(value); if (data !== null && typeof data === 'object') parsed = data; } catch { /* literal output */ } }
  const visibleBlocks = blocks.some(block => block.attachment !== undefined || block.content !== '');
  if (parsed !== undefined && (!visibleBlocks || blocks.length === 1 && blocks[0].type === 'text')) {
    return <JsonTree data={parsed} label="结果 JSON" labels={jsonLabels} collapsedStringLines={preview ? 3 : 12} stringWrapping={stringWrapping} />;
  }
  if (!visibleBlocks) return <pre className="trajectory-tool-output-fallback" data-error={error || undefined}
    data-empty={value === '无输出' || undefined}>{value || '无输出'}</pre>;
  return <div className={`trajectory-tool-text-output${preview ? ' trajectory-tool-output-preview' : ''}`} data-error={error || undefined}>
    {error && value && !blocks.some(block => block.type === 'text' && block.content !== '') && <pre>{value}</pre>}
    {blocks.map((block, index) => block.attachment
      ? <div className="trajectory-result-images" key={index}><ResultImage attachment={block.attachment} /></div>
      : block.content !== '' ? <pre key={index}>{block.content}</pre> : null)}
  </div>;
}
