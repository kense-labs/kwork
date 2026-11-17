/** Official menu resolution/presentation/contributions over the GPUI navigation and native file carriers. */
import { createRoot } from 'react-dom/client';
import { useSyncExternalStore } from 'react';
import { MenuRegistry } from './vendor/harness-context-menu/registry';
import { createMenuStore } from './vendor/harness-context-menu/store';
import { MenuOverlay } from './vendor/harness-context-menu/View';
import { menuTargetKey } from './vendor/harness-context-menu/target-key';
import { isContextMenuTrigger } from './vendor/harness-context-menu/external-trigger';
import { registerContextMenus, ContextMenuApplicationIcon } from './vendor/harness-context-menu/path-menus';
import { registerFileMenus } from './vendor/harness-context-menu/file-menus';
import { OpenInAppPathController } from './vendor/harness-context-menu/open-path';
import { parseFileAddress } from './vendor/harness-context-menu/file-address';
import { createContextMenuBindings } from './vendor/harness-images/trigger';
import { zh as pathLabels } from './vendor/harness-context-menu/path-locales';
import { zh as fileLabels } from './vendor/harness-images/file-locales';
import { MENU_TRANSITION_MS } from './vendor/harness-primitives/MenuSurface';
import { IconCopyOutlineRegular, IconDownloadOutlineRegular, IconSaveOutlineRegular,
  IconFolderOpenOutlineRegular, IconPanelLeftOutlineRegular, IconRightUpOutlineRegular } from './vendor/harness-primitives/icons';
import { invokeFileAction, nativeFileActions } from './native-file-actions';
import type { MarkdownMenuTarget } from './vendor/harness-primitives/markdown/MarkdownDelegate';
import type { ContextMenuTarget, MenuPoint, MenuResolution } from './vendor/harness-context-menu/types';

const store = createMenuStore().create();
const cleanups: (() => void)[] = [];
const directories = new Map<string, string>();
let root: ReturnType<typeof createRoot> | undefined;
let send: (session: string, action: string, value?: object) => void;
let sequence = 0;
let active: { target: ContextMenuTarget; point: MenuPoint; resolution: MenuResolution; unsubscribe: () => void } | undefined;
let returnFocus: HTMLElement | null = null;
let closeTimer: ReturnType<typeof setTimeout> | undefined;
let openingTimer: ReturnType<typeof setTimeout> | undefined;
let finishOpening: (() => void) | undefined;
let disposed = false;
const notify = (text: string, success = false, action?: {label:string;run:()=>void|Promise<void>;failureLabel:string}) => {
  if (disposed) return;
  const id = ++sequence;
  store.actions.notify({ id, text, success, ...action ? {action:{label:action.label,onClick:()=>{
    if (disposed || store.getSnapshot().notice?.id !== id) return;
    store.actions.notify(null);
    try { void Promise.resolve(action.run()).catch(() => notify(action.failureLabel)); }
    catch { notify(action.failureLabel); }
  }}} : {} });
};
const registry = new MenuRegistry(message => notify(message ?? '菜单加载失败，请重试'), 1000);
function cancelOpening() {
  clearTimeout(openingTimer); finishOpening?.(); finishOpening = undefined;
  active?.unsubscribe(); active?.resolution.dispose(); active = undefined;
}
function dismiss(replacing = false) {
  cancelOpening(); clearTimeout(closeTimer);
  if (store.getSnapshot().target === null) return;
  store.actions.exit(replacing);
  closeTimer = setTimeout(() => store.actions.close(), replacing ? MENU_TRANSITION_MS.replace : MENU_TRANSITION_MS.dismiss);
}
function show(target: ContextMenuTarget, point: MenuPoint) {
  if (active && Math.hypot(active.point.x-point.x, active.point.y-point.y) <= 4
      && menuTargetKey(active.target) === menuTargetKey(target)) { dismiss(); return; }
  const focused = document.activeElement;
  if (!(focused instanceof Element) || focused.closest('[data-context-menu]') === null)
    returnFocus = focused instanceof HTMLElement ? focused : null;
  dismiss(true);
  const resolution = registry.resolve(target);
  let displayed = false;
  const publish = () => { if (displayed) store.actions.items(resolution.source.getSnapshot()); };
  const occurrence = { resolution, unsubscribe: resolution.source.subscribe(publish), target, point };
  active = occurrence;
  const delay = new Promise<void>(resolve => { finishOpening = resolve; openingTimer = setTimeout(resolve, MENU_TRANSITION_MS.prepare); });
  void Promise.all([resolution.ready, delay]).then(() => {
    if (active !== occurrence) return;
    clearTimeout(closeTimer); store.actions.open(target, point); displayed = true; publish();
  });
}
function select(id: string) {
  const selected = active?.resolution;
  active?.unsubscribe(); active = undefined; dismiss();
  selected?.select(id); selected?.dispose();
}
const remoteResult = async (call: () => Promise<any>, signal?: AbortSignal) => {
  try {
    signal?.throwIfAborted(); const value = await call(); signal?.throwIfAborted();
    return {ok:true as const,value};
  } catch (error) { signal?.throwIfAborted(); return {ok:false as const,error}; }
};
const workspaceFile = (sessionId:string, path:string, read:boolean, signal?:AbortSignal) => remoteResult(async () => {
  const resolved = await invokeFileAction('resolve_session_document', {sessionId,path});
  signal?.throwIfAborted();
  if (resolved.cwd) directories.set(sessionId,resolved.cwd);
  return invokeFileAction('document_file', {path:resolved.path,read});
},signal);
const remote = {
  workspaceFiles: {
    stat: (session:string,path:string,signal:AbortSignal) => workspaceFile(session,path,false,signal),
    readBytes: (session:string,path:string,_range:object,signal:AbortSignal) => workspaceFile(session,path,true,signal),
  },
  session: {
    canOpenWorkspacePath: async () => ({ok:true as const,value:true}),
    workspacePathApplications: ({path}:{path:string},signal?:AbortSignal) => remoteResult(() => invokeFileAction('document_applications',{path}),signal),
    openWorkspacePath: ({path,action,application}:{path:string;action?:string;application?:string},signal?:AbortSignal) =>
      remoteResult(() => invokeFileAction('open_document_application',{path,reveal:action==='reveal',application}),signal),
  },
};
const paths = new OpenInAppPathController(remote);
const ctx: any = {
  effect: (setup:()=>()=>void) => cleanups.push(setup()),
  contextMenus: {register:registry.register.bind(registry),notify}, remote,
  sessions: {list:{getSnapshot:()=>({byId:Object.fromEntries([...directories].map(([id,cwd])=>[id,{cwd}]))})}},
  sidebarRightTabs: {get:()=>true},
  sidebarRight: {
    openResourceIn(session:string,address:string,{destination,params}:any) {
      const file = parseFileAddress(address);
      if (file) send(session,'file-menu',{path:file.path,destination,...params});
    },
    openTabIn(session:string,_tab:string,{params}:any) { send(session,'url-menu',params); },
  },
};
registerContextMenus(ctx,paths,(key,values) => {
  let value:string = pathLabels[key];
  for (const [key,replacement] of Object.entries(values??{})) value=value.replace(`{${key}}`,replacement);
  return value;
}, {apps:{getSnapshot:()=>['finder']},load:async()=>{}} as any, outcome => {
  if (outcome.kind==='failed') notify(pathLabels[`path.${outcome.failure}`]);
});
registerFileMenus(ctx,nativeFileActions,key=>fileLabels[key]);
const icons = {
  'open-in-app.reveal':IconFolderOpenOutlineRegular,'open-in-app.copy':IconCopyOutlineRegular,
  'open-in-app.sidebar':IconPanelLeftOutlineRegular,'open-in-app.float':IconRightUpOutlineRegular,
  'open-in-app.application':ContextMenuApplicationIcon,
  'file-actions.copy':IconCopyOutlineRegular,'file-actions.download':IconDownloadOutlineRegular,'file-actions.save':IconSaveOutlineRegular,
};
function Overlay() {
  return <MenuOverlay useStore={(selector:any)=>selector(useSyncExternalStore(store.subscribe,store.getSnapshot))}
    actions={store.actions} useCatalog={()=>[]} t={()=>'返回'} close={()=>dismiss()} select={select} returnFocus={()=>returnFocus}
    renderSlot={(name:string,props:any)=>{const Icon = name==='contextMenu.icon' ? icons[props.icon.key as keyof typeof icons] : undefined;return Icon ? <Icon {...props}/> : null;}} />;
}
const bindings = createContextMenuBindings(show,()=>!disposed);
export function prepareMarkdownMenu(session:string,workspace:string,callback:typeof send) {
  send=callback; if (workspace) directories.set(session,workspace);
  retainMarkdownMenu(new Set([session]));
  if (!root) { const holder=document.createElement('div');holder.dataset.chatContextMenu='';document.body.append(holder);root=createRoot(holder);root.render(<Overlay/>); }
}
export function markdownMenuBindings(session:string) {
  return (target:MarkdownMenuTarget) => {
    const {kind,...value}=target;
    return bindings.getTriggerProps({kind,value:{sessionId:session,...value}} as ContextMenuTarget);
  };
}
export function retainMarkdownMenu(sessions:Set<string>) {
  const session = active?.target.value && 'sessionId' in active.target.value ? active.target.value.sessionId : undefined;
  if (session && !sessions.has(session)) dismiss();
}
document.addEventListener('pointerdown',event=>{
  if (event.button===2 || event.target instanceof Node && isContextMenuTrigger(event.target)
      || event.target instanceof Element && event.target.closest('[role="menu"]')) return;
  dismiss();
});
document.addEventListener('contextmenu',event=>{if (!event.defaultPrevented) dismiss();});
window.addEventListener('keydown',event=>{
  if (active && store.getSnapshot().target===null && event.key==='Escape') {
    event.preventDefault();event.stopImmediatePropagation();dismiss();
  }
},true);
window.addEventListener('pagehide',()=>{
  disposed=true;cancelOpening();clearTimeout(closeTimer);registry.dispose();paths.dispose();
  for (const cleanup of cleanups) cleanup?.();
},{once:true});
