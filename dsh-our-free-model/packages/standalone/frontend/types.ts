export type Page = 'overview' | 'models' | 'usage' | 'channels' | 'eac' | 'settings' | 'connection'
export interface Model {
  id: string
  name: string
  channel: string
  routable: boolean
  hiddenByUser?: boolean
  hiddenByGroup?: boolean
  vision?: boolean
  reasoning?: boolean
  contextWindow?: number
  maxOutput?: number
  availabilitySource?: 'listing' | 'probe' | 'manual'
  detail?: string
  probedAt?: number
  latencyMs?: number
  availability?: string
  ttftMs?: number | null
}
export interface ModelTestResult { ok: boolean; text: string; latencyMs: number; ttftMs?: number; persisted?: boolean }
export interface ServiceSettings {
  enabled: boolean
  exposeRegionModels: boolean
  hiddenModelIds: string[]
  hideFreeModels: boolean
  hideEacModels: boolean
  streamRecovery: boolean
  standaloneProbe: boolean
  probeIntervalMinutes: number
  defaultMaxTokens: number
  standaloneLanEnabled?: boolean
  workbuddyValidationEnabled?: boolean
}
export interface WorkBuddyValidationState {
  enabled: boolean
  stopped: boolean
  busy: boolean
  last: { requestId: string; status: number; durationMs: number; at: string } | null
}
export interface LanState {
  enabled: boolean
  running: boolean
  port: number | null
  baseUrls: string[]
  error?: string | null
}
export interface Summary {
  version: string
  baseUrl: string
  dataDir: string
  networkMode: string
  automaticRefresh: boolean
  catalogSyncedAt: number
  settings: ServiceSettings
  lan?: LanState
  workbuddyValidation?: WorkBuddyValidationState
  channels: { state: string; error?: string }
  eacAuth?: { local?: boolean; authorized?: boolean; login?: string }
  catalog: Model[]
}
export interface Stats {
  requests: number
  requestFailures: number
  requestFailuresEstimated?: boolean
  logicalEstimated?: boolean
  turns: number
  failedTurns: number
  recoveredTurns: number
  days: { day: string; total: number }[]
  grand: { input: number; output: number; reasoning: number }
  models: UsageModel[]
  samples: { model?: string; ok?: boolean; error?: string; at?: number; ms?: number }[]
}
export interface UsageModel {
  model: string
  name: string
  calls: number
  turns: number
  input: number
  output: number
  reasoning: number
  failed: number
  failedTurns: number
  recoveredTurns: number
  avgTtftMs?: number | null
  tps?: number | null
}
export interface Snapshot { summary: Summary; stats: Stats }
export interface Host {
  navigate(page: Page): void
  refresh(): Promise<void>
  copy(value: string): Promise<void>
  getApiKey(): Promise<string>
  rotateKey(): Promise<string>
  getLanKey(): Promise<string>
  rotateLanKey(): Promise<string>
  exportBackup(): Promise<void>
  logout(): Promise<void>
  error(error: Error): void
  testModel(model: string, signal: AbortSignal): Promise<ModelTestResult>
  validateWorkBuddy(model: string, signal: AbortSignal): Promise<NonNullable<WorkBuddyValidationState['last']>>
  refreshModels(probe: boolean, signal: AbortSignal): Promise<void>
  saveSettings(settings: Partial<ServiceSettings>): Promise<ServiceSettings>
}
