//! Host-owned in-memory Session reading records for the local GPUI bridge.
//!
//! The ACP profile has no Typert/RemoteAccess stack. This keeps the official
//! SessionStatusService semantics while exposing them through the private,
//! main-window-only bridge instead of pretending a Remote namespace exists.

const emptyRecord = () => ({ completedTurn: 0, failed: false, readTurn: 0, manualUnread: false });

function failure(code, message) {
  return Object.assign(new Error(message), { code });
}

export default class SessionReadStatus {
  constructor(ctx, publish) {
    this.ctx = ctx;
    this.publish = publish;
    this.records = new Map();
    this.open = true;
    this.release = ctx.on('session/event', (session, event) => {
      if (event.type === 'turn/end') {
        this.complete(session.id, event.data.turn, event.data.reason.kind === 'error');
      }
    }, { global: true });
    ctx.effect(() => () => this.dispose(), 'session-read-status: discard records');
  }

  snapshot() {
    return [...this.records].map(([sessionId, record]) => ({ sessionId, ...record }));
  }

  complete(sessionId, turn, failed) {
    if (!this.open) return;
    const record = { ...(this.records.get(sessionId) ?? emptyRecord()), completedTurn: turn, failed };
    this.records.set(sessionId, record);
    this.publish({ sessionId, ...record });
  }

  async mark(sessionId, action) {
    this.assertOpen();
    if (this.ctx.get('sessions').get(sessionId) === undefined) {
      let stored;
      try {
        stored = await this.ctx.get('sessionPersistence').stat(sessionId);
      } catch (cause) {
        throw failure('session-status/unavailable', `无法读取会话：${String(cause)}`);
      }
      this.assertOpen();
      if (stored === undefined) throw failure('session-status/not-found', '会话不存在或已删除。');
    }
    if (!['read', 'unread', 'acknowledge'].includes(action?.type)) {
      throw failure('session-status/invalid-action', '无效的阅读状态操作。');
    }
    const current = this.records.get(sessionId) ?? emptyRecord();
    const throughTurn = action.type === 'unread' ? 0 : action.throughTurn;
    if (action.type !== 'unread' && (!Number.isSafeInteger(throughTurn) || throughTurn < 0)) {
      throw failure('session-status/invalid-action', '阅读位置无效。');
    }
    const next = {
      ...current,
      manualUnread: action.type === 'acknowledge' ? current.manualUnread : action.type === 'unread',
      readTurn: action.type === 'unread' ? current.readTurn
        : Math.max(current.readTurn, Math.min(throughTurn, current.completedTurn)),
    };
    this.records.set(sessionId, next);
    this.publish({ sessionId, ...next });
  }

  assertOpen() {
    if (!this.open) throw failure('session-status/unavailable', '阅读状态服务已断开，请重新连接。');
  }

  dispose() {
    if (!this.open) return;
    this.open = false;
    this.release();
    this.records.clear();
  }
}
