/**
 * Narrow a request to a renderable plan review, or return undefined to leave it
 * to the generic question flow.
 *
 * The card offers approval and a return to the composer for change requests.
 * It accepts one question carrying the plan as detail and the named approve
 * option, with at most one alternative and no multi-select. Larger choices
 * remain in the generic question flow.
 *
 * @param questions - the request's whole question batch.
 * @returns The narrowed review, or undefined when the generic flow owns it.
 */
export function planReviewOf(questions: readonly QuestionItem[]): PlanReview | undefined {
  if (questions.length !== 1) return undefined
  // Length-checked above; the index read is the narrowing tax, not a guess.
  const question = questions[0] as QuestionItem
  const intent = question.intent
  if (intent?.kind !== 'plan-review' || question.detail === undefined) return undefined
  if (question.multiSelect === true) return undefined
  const options = question.options ?? []
  if (options.length > 2) return undefined
  const approve = options.find(option => option.label === intent.approve)
  if (approve === undefined) return undefined
  const decline = options.find(option => option.label !== intent.approve)
  return {
    id: question.id,
    question: question.question,
    plan: question.detail,
    ...(intent.callId === undefined ? {} : { callId: intent.callId }),
    approve,
    ...(decline === undefined ? {} : { decline }),
  }
}


import type { ReactNode } from 'react'
import type { QuestionDraftProgress, QuestionDraftState } from '../draft-store.ts'

export interface QuestionOption { label: string; description?: string }
export interface QuestionItem {
  id: string
  question: string
  header?: string
  detail?: string
  multiSelect?: boolean
  options?: QuestionOption[]
  intent?: { kind: 'plan-review'; approve: string; callId?: string }
}
type AskUserQuestionAnswerItem = { id: string; selected: string[]; custom?: string }
export type QuestionAnswer = { answers: AskUserQuestionAnswerItem[] }
export interface PlanReview {
  id: string
  question: string
  plan: string
  callId?: string
  approve: QuestionOption
  decline?: QuestionOption
}
export interface QuestionCardSnapshot {
  state: 'open' | 'continued'
  waitState: 'counting' | 'focused' | 'editing' | 'waiting' | 'continued'
  countdown?: { remainingMs: number; running: boolean }
  channel: 'waterfall' | 'rpc' | 'none'
  closed: boolean
}
export interface PendingQuestion {
  key: string
  questions: QuestionItem[]
  review?: AskUserQuestionAnswerItem[]
  dismissal: 'hide' | 'cancel'
  liveKeys(): readonly string[]
  takeTime(): void
  engage(): void
  holdFocus(): void
  releaseFocus(): void
  snapshot(): QuestionCardSnapshot
  answer(answer: QuestionAnswer): Promise<void>
  dismiss(): Promise<void>
}

export interface QuestionComposerProps {
  matched: PendingQuestion
  t(key: string, values?: Record<string, string | number>): string
  renderSlot(name: string, owner: { review: PlanReview; requestKey: string }): ReactNode
  useStore<Value>(select: (state: QuestionDraftState) => Value): Value
  useQuestionCard<Value>(key: string, select: (state: QuestionCardSnapshot) => Value): Value
  actions: {
    replace(key: string, progress: QuestionDraftProgress): void
    clear(key: string): void
    prune(keys: readonly string[]): void
  }
}
