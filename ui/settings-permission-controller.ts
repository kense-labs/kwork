import { createSnapshotStore } from './vendor/harness-settings-models/snapshot-store';
import { PermissionPresetSettingsController } from './vendor/harness-settings-permission/settings-store';

/** Project the native, allowlisted defaults endpoint into the official controller. */
export function createPermissionController(invoke: (request: object) => Promise<any>) {
  let active = true;
  let revision = 0;
  let readError: unknown;
  const mirror = createSnapshotStore<{ view?: any; error: string | null }>({ error: null });
  const catalog = createSnapshotStore<{ value: any }>({ value: null });
  const call = async (request: object) => {
    if (!active) throw new Error('服务连接已变化，请重试。');
    const reply = await invoke(request);
    if (!active) throw new Error('服务连接已变化，请重试。');
    return reply;
  };
  const describe = {
    ...mirror,
    ensure: async () => {
      const current = ++revision;
      try {
        const reply = await call({ operation: 'read' });
        if (current !== revision) return;
        readError = undefined;
        catalog.set({ value: reply.catalog });
        mirror.set({ view: reply.view, error: null });
      } catch (error) {
        if (!active || current !== revision) return;
        readError = error;
        mirror.set({ ...mirror.getSnapshot(), error: error instanceof Error ? error.message : String(error) });
      }
    },
    acceptView: (view: any) => { ++revision; readError = undefined; mirror.set({ view, error: null }); },
  };
  const controller = new PermissionPresetSettingsController(describe as any, { remote: { settings: {
    mutate: async (_ns: string, ops: any[], expected: number) => {
      try {
        const preset = ops[0].value;
        const reply = await call({ operation: 'set', preset, revision: expected,
          acknowledged: preset === 'danger-full-access' });
        catalog.set({ value: reply.catalog });
        return { ok: true, value: reply.view };
      } catch (error) {
        return { ok: false, error: error instanceof Error ? error : new Error(String(error)) };
      }
    },
  } } } as any, { store: catalog, load: async () => { if (readError) throw readError; return catalog.getSnapshot().value; } } as any);
  return { controller, load: () => controller.load(), select: (preset: string) => controller.select(preset),
    dispose() { active = false; ++revision; controller.dispose(); } };
}
