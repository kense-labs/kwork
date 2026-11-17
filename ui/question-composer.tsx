/** Bind the official question composer to KWork's ACP request lifecycle. */
import { useMemo, useRef, useSyncExternalStore } from 'react';
import { createSnapshotStore } from './vendor/harness-settings-models/snapshot-store';
import { QuestionComposer } from './vendor/harness-questions/QuestionComposer';
import { questionDraftStore, type QuestionDraftState, type QuestionDraftProgress } from './vendor/harness-questions/draft-store';
import { zh as questionText } from './vendor/harness-questions/locales';
import { zh as common } from './vendor/harness-locale/zh';
import type { PendingQuestion, QuestionItem, QuestionAnswer, QuestionCardSnapshot } from './vendor/harness-questions/contract/slots';
import { IconChevronRightOutlineRegular } from './vendor/harness-primitives/icons';
import { MarkdownDelegateProvider } from './vendor/harness-primitives/markdown/MarkdownDelegate';
import { invokeFileAction, openExternalFileLink } from './native-file-actions';
import planCss from './vendor/harness-plan/PlanPreview.module.css';

export type Interaction = {
  kind: 'question' | 'plan';
  requestId: string;
  questions: QuestionItem[];
  review?: QuestionAnswer['answers'];
  canHide: boolean;
  liveKeys: string[];
  snapshot: QuestionCardSnapshot;
};

const dictionary: Record<string, string> = { ...common, ...questionText };
const translate = (key: string, values: Record<string, string | number> = {}) =>
  (dictionary[key] ?? key).replace(/\{([^}]+)\}/g, (match, name) => String(values[name] ?? match));
const external = (url: string) => { void openExternalFileLink(url).catch(() => {}); };
const submitting = new Set<string>();
export function questionSubmitting(session: string, requestId: string) {
  return submitting.has(JSON.stringify([session, requestId]));
}

type Send = (session: string, action: string, value: object) => void;
type DraftStore = ReturnType<typeof createDraftStore>;

const stores = new Map<string, DraftStore>();
function createDraftStore(session: string) {
  const key = `${questionDraftStore.persist}.${session}`;
  let initial = questionDraftStore.init();
  try {
    const stored = JSON.parse(localStorage.getItem(key) ?? 'null');
    if (stored && typeof stored.progressByRequest === 'object' && stored.progressByRequest !== null)
      initial = stored;
  } catch { /* a missing or malformed draft is recoverable */ }
  const store = createSnapshotStore(initial);
  const update = (action: (draft: QuestionDraftState) => void) => {
    const draft = { progressByRequest: { ...store.getSnapshot().progressByRequest } };
    action(draft);
    store.set(draft);
    try { localStorage.setItem(key, JSON.stringify(draft)); } catch { /* still usable in memory */ }
  };
  return { store, actions: {
    replace: (key: string, progress: QuestionDraftProgress) =>
      update(draft => questionDraftStore.actions.replace(draft, key, progress)),
    clear: (key: string) => update(draft => questionDraftStore.actions.clear(draft, key)),
    prune: (keys: readonly string[]) => update(draft => questionDraftStore.actions.prune(draft, keys)),
  } };
}

export function clearQuestionDraft(session: string, request: string) {
  const store = stores.get(session) ?? createDraftStore(session);
  store.actions.clear(request);
}

function draftsFor(session: string) {
  let draft = stores.get(session);
  if (!draft) {
    if (stores.size >= 64) stores.delete(stores.keys().next().value!);
    draft = createDraftStore(session);
    stores.set(session, draft);
  }
  return draft;
}

export function InteractionBody({session, interaction, send}: {session: string; interaction: Interaction; send: Send}) {
  const {requestId, kind, snapshot} = interaction;
  const drafts = draftsFor(session);
  const state = useSyncExternalStore(drafts.store.subscribe, drafts.store.getSnapshot);
  const latest = useRef(interaction);
  latest.current = interaction;
  const matched = useMemo<PendingQuestion>(() => {
    let answered = false;
    // A React focus callback can run while WKWebView is measuring a WASM frame.
    const afterLayout = (action: string, value: object) => {
      setTimeout(() => send(session, action, { requestId, ...value }), 0);
    };
    return {
      key: requestId,
      questions: interaction.questions,
      review: interaction.review,
      dismissal: interaction.canHide ? 'hide' : 'cancel',
      liveKeys: () => latest.current.liveKeys,
      snapshot: () => latest.current.snapshot,
      holdFocus: () => afterLayout('question-focus', { focused: true }),
      releaseFocus: () => afterLayout('question-focus', { focused: false }),
      engage: () => afterLayout('question-hold', { editing: true }),
      takeTime: () => afterLayout('question-hold', {}),
      async answer(answer) {
        if (kind === 'plan') {
          await invokeFileAction('respond_plan_review', {sessionId:session, requestId, decision:'approve'});
          return;
        }
        const channel = latest.current.snapshot.channel;
        const pendingKey = JSON.stringify([session, requestId]);
        submitting.add(pendingKey);
        try {
          await invokeFileAction('respond_question', {sessionId:session, requestId, decision:'answer', answers:answer.answers});
          answered = channel === 'rpc';
        } finally { submitting.delete(pendingKey); }
      },
      async dismiss() {
        if (kind === 'plan') {
          await invokeFileAction('respond_plan_review', {sessionId:session, requestId, decision:'discuss'});
        } else if (latest.current.canHide) {
          send(session, 'question-hide', {requestId, settled: answered});
        } else {
          await invokeFileAction('respond_question', {sessionId:session, requestId, decision:'cancel'});
        }
      },
    };
  }, [kind, requestId, session, send]);
  return <MarkdownDelegateProvider openExternalLink={external}>
    <QuestionComposer matched={matched} t={translate} useStore={select => select(state)}
      actions={drafts.actions} useQuestionCard={(_, select) => select(snapshot)}
      renderSlot={() => <button className={planCss.reviewLink} type="button"
        title="在侧边栏打开计划" aria-label="在侧边栏打开计划"
        onClick={() => send(session, 'plan-preview', {requestId})}>查看全文<IconChevronRightOutlineRegular size={14} /></button>} />
  </MarkdownDelegateProvider>;
}
