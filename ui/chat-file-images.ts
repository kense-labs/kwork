/** Session-scoped original images for the upstream Markdown image/hover components. */
import { useEffect, useMemo, useState } from 'react';
import mime from 'mime';
import { markdownImagePaths } from './markdown-navigation';
import { invokeFileAction } from './native-file-actions';

export function useMarkdownFileImages(session: string, text: string, streaming: boolean,
  seeds: readonly {path:string;url:string}[]): ReadonlyMap<string,string> {
  const state = useMemo(() => ({urls:new Map<string,string>(), owned:new Set<string>(), failed:new Set<string>()}), [session]);
  const [, changed] = useState(0);
  const paths = useMemo(() => streaming ? [] : markdownImagePaths(text), [text,streaming]);
  // A canonical signature avoids restarting reads when a stream only appends prose.
  const signature = JSON.stringify(paths);
  const seeded = new Map(seeds.map(image => [image.path,image.url]));
  const seedSignature = JSON.stringify(seeds);
  useEffect(() => {
    const lifetime = new AbortController();
    const pending = paths.filter(path => !seeded.has(path) && !state.urls.has(path) && !state.failed.has(path));
    const read = async () => {
      while (pending.length && !lifetime.signal.aborted) {
        const path = pending.shift()!;
        try {
          const resolved = await invokeFileAction('resolve_session_document', {sessionId:session,path});
          if (lifetime.signal.aborted) return;
          const result = await invokeFileAction('document_file', {path:resolved.path,read:true});
          if (lifetime.signal.aborted) return;
          const url = URL.createObjectURL(new Blob([new Uint8Array(result.data)], {type:mime.getType(path) ?? ''}));
          state.owned.add(url);state.urls.set(path,url);changed(value=>value+1);
        } catch {
          if (!lifetime.signal.aborted) state.failed.add(path);
        }
      }
    };
    for (let index=0;index<Math.min(4,pending.length);index++) void read();
    return () => lifetime.abort();
  }, [state,session,signature,seedSignature]);
  useEffect(() => () => {
    for (const url of state.owned) URL.revokeObjectURL(url);
    state.owned.clear();state.urls.clear();state.failed.clear();
  }, [state]);
  return new Map([...state.urls,...seeded]);
}
