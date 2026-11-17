/** Host data shape consumed by the unchanged upstream Platform implementation. */
export interface PlatformSession {
  origin: string
  token: string
  userId: string | null
  embeddedPageDist?: string | null
  requestHeaders?: Record<string, string>
}

export interface AccountClientMetadata {
  version: string
  locale: string
  timezoneOffsetSeconds: number
}
