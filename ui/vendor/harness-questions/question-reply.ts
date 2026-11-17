/** One asked question as echoed in the reply payload. */
export interface QuestionReplyQuestion {
  readonly id: string
  readonly question: string
  readonly header?: string
}

/** One structured answer as echoed in the reply payload. */
export interface QuestionReplyAnswer {
  readonly id: string
  readonly selected: readonly string[]
  readonly custom?: string
}

/** Presentation data of one submitted answer batch. */
export interface QuestionReplyData {
  readonly callId: string
  /** A submitted answer batch, including individually skipped questions. */
  readonly outcome: 'answered'
  readonly questions: readonly QuestionReplyQuestion[]
  readonly answers: readonly QuestionReplyAnswer[]
  /** Model-facing text, shown when the payload is unreadable. */
  readonly text: string
  readonly time?: number
}

/**
 * Answer values of one question in display order: the selected option labels
 * followed by a non-blank custom answer.
 * @param data - Projected reply holding the recorded answers.
 * @param id - Question id to read.
 * @returns The values, empty when the user skipped that question.
 */
export function replyAnswerValues(data: QuestionReplyData, id: string): string[] {
  const answer = data.answers.find(item => item.id === id)
  const custom = answer?.custom?.trim() ?? ''
  return [...(answer?.selected ?? []), ...(custom === '' ? [] : [custom])]
}

/**
 * Clipboard text of one answer batch: every question with the answer the user
 * gave, without question details or unselected options.
 * @param data - Projected reply to copy.
 * @param t - Bound `question` namespace translator owning the answer labels.
 * @returns One block per question, or the model-facing text when the payload was unreadable.
 */
export function replyClipboardText(data: QuestionReplyData, t: (key: string) => string): string {
  if (data.questions.length === 0) return data.text
  return data.questions.map((question) => {
    const values = replyAnswerValues(data, question.id)
    const heading = question.header && question.header !== question.question
      ? `${question.header} — ${question.question}`
      : question.question
    const answer = values.length === 0
      ? t('reply.skipped')
      : `${t('reply.answerLabel')}${values.join(', ')}`
    return `${heading}\n${answer}`
  }).join('\n\n')
}
