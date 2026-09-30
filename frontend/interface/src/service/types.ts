export interface AutoReloadConfig {
  enabled: boolean
  onProxyChange: boolean
  onProfileChange: boolean
  onModeChange: boolean
}

export interface SystemProxy {
  enable: boolean
  server: string
  bypass: string
}

export interface LogMessage {
  type: string
  time?: string
  payload: string
}

export interface ProviderRules {
  behavior: string
  format: string
  name: string
  ruleCount: number
  type: string
  updatedAt: string
  vehicleType: string
}

export interface Traffic {
  up: number
  down: number
}

export interface Memory {
  inuse: number
  oslimit: number
}

export interface EnvInfos {
  os: string
  arch: string
  core: { [key: string]: string }
  device: {
    cpu: Array<string>
    memory: string
  }
  build_info: { [key: string]: string }
}

export interface InspectUpdater {
  id: number
  state:
    | 'idle'
    | 'downloading'
    | 'decompressing'
    | 'replacing'
    | 'restarting'
    | 'done'
    | { failed: string }
  downloader: {
    state:
      | 'idle'
      | 'downloading'
      | 'waiting_for_merge'
      | 'merging'
      | { failed: string }
      | 'finished'
    downloaded: number
    total: number
    speed: number
    chunks: Array<{
      state: 'idle' | 'downloading' | 'finished'
      start: number
      end: number
      downloaded: number
      speed: number
    }>
    now: number
  }
}
