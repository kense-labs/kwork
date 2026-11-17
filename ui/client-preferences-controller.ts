type Namespace = { ns: string; revision: number; value: Record<string, unknown> };
type Snapshot = { writable: boolean; namespaces: Namespace[] };
type Request = { operation: 'read' } | { operation: 'set'; namespace: string; revision: number; changes: Record<string, unknown> };

/** Host acceptance and optimistic theme writes have different lifetimes. */
export function createClientPreferencesController(
  invoke: (request: Request) => Promise<Snapshot>,
  publish: (snapshot: Snapshot & { available: string[] }) => void,
) {
  let alive = true, serial = 0, readSerial = 0;
  let snapshot: Snapshot = { writable: false, namespaces: [] };
  let tail = Promise.resolve();
  const pending = new Map<string, number>();
  const emit = () => {
    if (alive) publish({ writable: snapshot.writable, available: snapshot.namespaces.map(row => row.ns),
      namespaces: snapshot.namespaces.filter(row => row.ns !== 'ui-theme' || !pending.get(row.ns)) });
  };
  const load = async () => {
    const version = serial, read = ++readSerial;
    const next = await invoke({ operation: 'read' });
    if (alive && version === serial && read === readSerial) { snapshot = next; emit(); }
  };
  const set = (namespace: string, changes: Record<string, unknown>) => {
    if (!alive) return Promise.reject(new Error('Client preference connection is closed'));
    const fields = structuredClone(changes);
    pending.set(namespace, (pending.get(namespace) ?? 0) + 1);
    serial++;
    const task = tail.then(async () => {
      if (!alive) throw new Error('Client preference connection is closed');
      const current = snapshot.namespaces.find(row => row.ns === namespace);
      if (!snapshot.writable || !current) throw new Error('Client preferences are unavailable or read-only');
      serial++;
      try {
        const next = await invoke({ operation: 'set', namespace, changes: fields, revision: current.revision });
        if (alive) snapshot = next;
      } catch (error) {
        try { const next = await invoke({ operation: 'read' }); if (alive) snapshot = next; }
        catch { /* Keep the last confirmed Host snapshot if recovery is offline. */ }
        throw error;
      } finally { serial++; }
    }).finally(() => { pending.set(namespace, (pending.get(namespace) ?? 1) - 1); emit(); });
    tail = task.catch(() => {});
    return task;
  };
  return { load, set, dispose() { alive = false; serial++; } };
}
