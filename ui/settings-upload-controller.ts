import { createSnapshotStore } from './vendor/harness-settings-models/snapshot-store';
import { UploadPreference } from './vendor/harness-settings-upload/upload-preference';

type Snapshot = { available: boolean; status?: string; writable?: boolean; value?: { enabled: boolean }; revision?: number };
export function createUploadController(invoke: (request: object) => Promise<Snapshot>) {
  const form = createSnapshotStore<Snapshot>({ available: false, status: 'idle' });
  let active = true;
  let request = 0;
  const load = async () => {
    const current = ++request;
    try {
      const snapshot = await invoke({ operation: 'read' });
      if (active && current === request) form.set(snapshot);
    } catch {
      if (active && current === request) form.set({ ...form.getSnapshot(), status: 'error', writable: false });
    }
  };
  const preference = new UploadPreference({ ...form, set: async (_field: string, enabled: boolean) => {
    const before = form.getSnapshot();
    if (!active || !before.available || !before.writable || before.status !== 'ready') return false;
    ++request;
    try {
      const snapshot = await invoke({ operation: 'set', enabled, revision: before.revision });
      if (!active) return false;
      ++request;
      form.set(snapshot);
      return snapshot.available && snapshot.value?.enabled === enabled;
    } catch {
      // Re-read conflicts or refusals; the accepted switch never flips optimistically.
      if (active) await load();
      return false;
    }
  } } as any);
  return { form, preference, load, dispose() { active = false; ++request; } };
}
