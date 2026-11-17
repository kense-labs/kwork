import type { PlanKey } from './locales.ts'
import type { zh } from '../harness-locale/zh.ts'

export type RemoteFailure = { code: string; message: string }
export type TranslateNS<_Namespace extends 'plan'> = (key: PlanKey | keyof typeof zh) => string
export type PropsLocale<Namespace extends 'plan'> = { t: TranslateNS<Namespace> }

export type PlanDocument = { title: string; markdown: string }
export type PlanResource = {
  status: 'none' | 'loading' | 'ready' | 'failed'
  value?: PlanDocument & { callId: string }
  failure?: RemoteFailure
}
export type PropsRuntime<_Slot extends string> = {
  useTabInfo: () => { tab: { title: string; navigation: { address: string; params?: { planReview: PlanDocument } } } }
  useResource: <_Protocol extends 'plan'>(address: string) => PlanResource
}
