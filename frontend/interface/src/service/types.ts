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

// eslint-disable-next-line @typescript-eslint/no-namespace
export namespace Connection {
  export interface Item {
    id: string
    metadata: Metadata
    upload: number
    download: number
    start: string
    chains: string[]
    rule: string
    rulePayload: string
  }

  export interface Metadata {
    network: string
    type: string
    host: string
    sourceIP: string
    sourcePort: string
    destinationPort: string
    destinationIP?: string
    destinationIPASN?: string
    process?: string
    processPath?: string
    dnsMode?: string
    dscp?: number
    inboundIP?: string
    inboundName?: string
    inboundPort?: string
    inboundUser?: string
    remoteDestination?: string
    sniffHost?: string
    specialProxy?: string
    specialRules?: string
  }

  export interface Response {
    downloadTotal: number
    uploadTotal: number
    memory?: number
    connections?: Item[]
  }
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
