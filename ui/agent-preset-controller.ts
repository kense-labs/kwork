/** Connection-owned adapters for the official preset controllers. */
import { AgentPresetSectionController } from './vendor/harness-agent-presets/section-store';
import { AgentPresetSettingsController } from './vendor/harness-agent-presets/settings-store';
import { AgentPresetSeatController, type AgentPresetStage } from './vendor/harness-agent-presets/seat-store';
import { createSnapshotStore } from './vendor/harness-settings-models/snapshot-store';

type Request = Record<string, unknown>;
type Session = { id: string; blank: boolean; projectionValues: { agentPreset?: string } };
type Binding = { summary?: Session; localBlank: boolean; revision: number; seat: AgentPresetSeatController };
type Result = { ok: true; value: any } | { ok: false; error: { message: string; code: string; details: {} } };

export class AgentPresetConnection {
  active = true;
  developerTools: boolean | undefined;
  currentId: string | undefined;
  readonly staged: AgentPresetStage = { id: undefined, introduce: false };
  readonly sessions = createSnapshotStore<{ byId: Record<string, Session> }>({ byId: {} });
  readonly bindings = new Map<string, Binding>();
  readonly section: AgentPresetSectionController;
  readonly roster: AgentPresetSettingsController;
  readonly unbound: AgentPresetSeatController;
  private document = { status: 'idle', writable: false, revision: undefined as number | undefined };
  private listPending: Promise<Result> | undefined;
  private reconcileRequested = 0;
  private reconcilePending: Promise<void> | undefined;
  private introduceCreator: { previous?: string; reuse?: string } | undefined;
  private readonly context: any;

  constructor(private readonly invoke: (request: Request) => Promise<any>, t: any) {
    this.context = {
      remote: {
        agentPresets: {
          list: () => this.list(),
          read: (id: string) => this.request({ operation: 'read', id }),
          select: async (sessionId: string, id: string) => {
            const binding = this.bindings.get(sessionId);
            if (!binding || this.currentId !== sessionId) return this.refusal('会话已更换，请重试。');
            const revision = ++binding.revision;
            const result = await this.request({ operation: 'select', sessionId, id });
            if (!result.ok) return result;
            if (binding.revision === revision) this.publish(binding, {
              id: sessionId, blank: binding.localBlank,
              projectionValues: { agentPreset: result.value.agentPreset },
            });
            return { ok: true, value: result.value.agentPreset };
          },
        },
        settings: { update: (_namespace: string, changes: { selectedDefault: string }, revision?: number) =>
          this.request({ operation: 'default', id: changes.selectedDefault, revision: revision ?? this.document.revision }) },
      },
      configForms: { get: () => ({ getSnapshot: () => this.document }) },
      locale: { bind: () => t },
    };
    this.section = new AgentPresetSectionController(this.context);
    this.roster = new AgentPresetSettingsController(this.context);
    this.unbound = new AgentPresetSeatController(this.context, () => undefined, this.staged);
  }

  private refusal(message: string): Result { return { ok: false, error: { message, code: 'kwork/preset-unavailable', details: {} } }; }
  private async request(request: Request): Promise<Result> {
    if (!this.active) return this.refusal('连接已更换，请重试。');
    try {
      const value = await this.invoke(request);
      if (!this.active) return this.refusal('连接已更换，请重试。');
      if ((request.operation === 'list' || request.operation === 'default')
          && (this.document.revision === undefined || value.revision === undefined || value.revision >= this.document.revision)) {
        this.document = { status: 'ready', writable: value.writable === true, revision: value.revision };
      }
      return { ok: true, value };
    } catch (error) { return this.refusal(error instanceof Error ? error.message : String(error)); }
  }
  private list(): Promise<Result> {
    return this.listPending ??= this.request({ operation: 'list' }).finally(() => { this.listPending = undefined; });
  }
  private publish(binding: Binding, summary: Session) {
    if (!this.active) return;
    binding.summary = summary;
    this.sessions.set({ byId: { ...this.sessions.getSnapshot().byId, [summary.id]: summary } });
  }
  get seat(): AgentPresetSeatController { return this.bindings.get(this.currentId ?? '')?.seat ?? this.unbound; }

  async setSession(id: string | undefined, blank: boolean, refresh = false): Promise<void> {
    const changed = this.currentId !== id;
    this.currentId = id;
    if (!id) { await this.unbound.load(); return; }
    let binding = this.bindings.get(id);
    const wasBlank = binding?.localBlank;
    if (!binding) {
      const own: Binding = { localBlank: blank, revision: 0, seat: undefined! };
      own.seat = new AgentPresetSeatController(this.context,
        () => this.active && this.currentId === id ? own.summary : undefined, this.staged);
      this.bindings.set(id, binding = own);
    }
    binding.localBlank = blank;
    if (binding.summary && !blank) this.publish(binding, { ...binding.summary, blank: false });
    if (!changed && wasBlank === blank && !refresh && binding.summary) return;
    const revision = ++binding.revision;
    const result = await this.request({ operation: 'session', sessionId: id });
    if (!this.active || binding.revision !== revision || !result.ok) return;
    this.publish(binding, { id, blank: blank && result.value.canSelect === true,
      projectionValues: typeof result.value.agentPreset === 'string' ? { agentPreset: result.value.agentPreset } : {} });
    const creator = this.introduceCreator;
    if (creator && (id !== creator.previous || id === creator.reuse) && result.value.agentPreset === 'cordis') {
      this.introduceCreator = undefined;
      if (this.currentId === id) binding.seat.stage('cordis', true);
    }
    await binding.seat.load();
  }

  async refresh(): Promise<void> {
    if (!this.active) return;
    await Promise.all([this.roster.load(), this.section.load(), this.unbound.load(),
      ...[...this.bindings.values()].map(binding => binding.seat.load())]);
    await this.reconcile();
  }
  setDeveloperTools(enabled: boolean | undefined): Promise<void> {
    if (this.developerTools === enabled) return this.reconcilePending ?? Promise.resolve();
    this.developerTools = enabled;
    if (enabled === false) {
      this.staged.id = undefined; this.staged.introduce = false;
      void this.unbound.apply();
      for (const binding of this.bindings.values()) void binding.seat.apply();
    }
    return this.reconcile();
  }
  reconcile(): Promise<void> {
    this.reconcileRequested++;
    return this.reconcilePending ??= Promise.resolve().then(async () => {
      try {
        for (;;) {
          const revision = this.reconcileRequested;
          await this.section.reconcileCodingTools(() => this.active && this.developerTools === false);
          if (!this.active || this.reconcileRequested === revision) return;
        }
      } finally { this.reconcilePending = undefined; }
    });
  }
  async makeDefault(id: string): Promise<void> {
    const sessionId = this.currentId;
    const binding = sessionId === undefined ? undefined : this.bindings.get(sessionId);
    const captured = binding?.seat.blankSessionId();
    await this.section.makeDefault(id, async selected => {
      if (!this.active || captured === undefined || this.currentId !== captured || this.bindings.get(captured) !== binding) return;
      return binding!.seat.syncBlankSession(captured, selected);
    });
    if (this.active) await Promise.all([this.roster.load(), this.seat.load()]);
  }
  creatorDraft(): { id: string; reuseSession?: string } {
    const current = this.bindings.get(this.currentId ?? '')?.summary;
    const reuse = current?.blank && current.projectionValues.agentPreset === 'cordis' ? current.id : undefined;
    this.introduceCreator = { previous: this.currentId, reuse };
    return { id: 'cordis', ...(reuse ? { reuseSession: reuse } : {}) };
  }
  dispose(): void {
    this.active = false; this.section.closeView(); this.staged.id = undefined; this.staged.introduce = false;
  }
}
