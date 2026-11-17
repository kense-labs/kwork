/** Timeline-only host types; presentation and projection are upstream source. */
import type { TrajectoryTranslate } from './locales';
export type TrajectoryCellKind = 'system' | 'user' | 'context' | 'compacted' | 'message' | 'tool' | 'subtool';
export interface TrajectoryCellProps {
  index: number;
  kind: TrajectoryCellKind;
  text: string;
  requestOnly?: boolean;
  isError?: boolean;
  timeSeconds: number | null;
  startedAt: number | null;
  assistantMetrics?: AssistantMetricDetail;
}
export interface AssistantMetricDetail {
  timingRecorded: boolean
  stepStartTime: number | null
  firstTokenTime: number | null
  completedTime: number | null
  usageProvided: boolean
  outputTokens: number | null
}

export function formatDurationMillis(
  milliseconds: number | null,
  t: TrajectoryTranslate,
): string {
  if (milliseconds === null || !Number.isFinite(milliseconds)) return '—'
  const integer = String(Math.round(milliseconds))
  return t('unit.milliseconds', {
    value: integer.replace(/\B(?=(\d{3})+(?!\d))/g, ','),
  })
}
