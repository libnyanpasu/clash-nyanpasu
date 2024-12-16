import { Clash } from './clash'

export interface VergeConfig {
  app_log_level?: 'trace' | 'debug' | 'info' | 'warn' | 'error' | string
  language?: string
  clash_core?:
    | 'mihomo'
    | 'mihomo-alpha'
    | 'clash-rs'
    | 'clash-rs-alpha'
    | 'clash'
  theme_mode?: 'light' | 'dark' | 'system'
  theme_blur?: boolean
  traffic_graph?: boolean
  enable_memory_usage?: boolean
  lighten_animation_effects?: boolean
  enable_auto_check_update?: boolean
  enable_tun_mode?: boolean
  enable_auto_launch?: boolean
  enable_service_mode?: boolean
  enable_silent_start?: boolean
  enable_system_proxy?: boolean
  enable_random_port?: boolean
  verge_mixed_port?: number
  enable_proxy_guard?: boolean
  proxy_guard_interval?: number
  system_proxy_bypass?: string
  web_ui_list?: string[]
  hotkeys?: string[]
  theme_setting?: {
    primary_color?: string
    secondary_color?: string
    primary_text?: string
    secondary_text?: string
    info_color?: string
    error_color?: string
    warning_color?: string
    success_color?: string
    font_family?: string
    css_injection?: string
    page_transition_duration?: number
  }
  max_log_files?: number
  auto_close_connection?: boolean
  default_latency_test?: string
  enable_clash_fields?: boolean
  enable_builtin_enhanced?: boolean
  proxy_layout_column?: number
  clash_tray_selector?: 'normal' | 'hidden' | 'submenu'
  clash_strategy?: {
    external_controller_port_strategy: 'fixed' | 'random' | 'allow_fallback'
  }
  tun_stack?: 'system' | 'gvisor' | 'mixed'
  always_on_top?: boolean
}

export interface ClashInfo {
  port?: number
  server?: string
  secret?: string
}
export interface ClashConfig {
  port: number
  mode: string
  ipv6: boolean
  'socket-port': number
  'allow-lan': boolean
  'log-level': string
  'mixed-port': number
  'redir-port': number
  'socks-port': number
  'tproxy-port': number
  'external-controller': string
  secret: string
}

export namespace Profile {
  export interface Config {
    current: string[]
    chain: string[]
    valid: string[]
    items: Item[]
  }

  export const Template = {
    merge: `# Clash Nyanpasu Merge Template (YAML)
# Documentation on https://nyanpasu.elaina.moe/
# Set the default merge strategy to recursive merge. 
# Enable the old mode with the override__ prefix. 
# Use the filter__ prefix to filter lists (removing unwanted content). 
# All prefixes should support accessing maps or lists with a.b.c syntax.
`,
    javascript: `// Clash Nyanpasu JavaScript Template
// Documentation on https://nyanpasu.elaina.moe/

/** @type {config} */
export default function (profile) {
  return profile;
}
`,
    luascript: `-- Clash Nyanpasu Lua Script Template
-- Documentation on https://nyanpasu.elaina.moe/

return config;
`,
    profile: `# Clash Nyanpasu Profile Template
# Documentation on https://nyanpasu.elaina.moe/

proxies:

proxy-groups:

rules:
`,
  }

  export const Type = {
    Local: 'local',
    Remote: 'remote',
    Merge: 'merge',
    JavaScript: {
      script: 'javascript',
    },
    LuaScript: {
      script: 'lua',
    },
  } as const

  export interface Item {
    uid: string
    type?: (typeof Type)[keyof typeof Type]
    name?: string
    desc?: string
    file?: string
    url?: string
    updated?: number
    selected?: {
      name?: string
      now?: string
    }[]
    extra?: {
      upload: number
      download: number
      total: number
      expire: number
    }
    option?: Option
    chain?: string[]
  }

  export interface Option {
    user_agent?: string
    with_proxy?: boolean
    self_proxy?: boolean
    update_interval?: number
  }
}

export interface SystemProxy {
  enable: boolean
  server: string
  bypass: string
}

export interface Proxies {
  direct: Clash.Proxy
  global: Clash.Proxy<Clash.Proxy>
  groups: Clash.Proxy<Clash.Proxy>[]
  proxies: Clash.Proxy[]
  records: {
    [key: string]: Clash.Proxy
  }
}

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

export interface ProviderItem {
  name: string
  type: string
  proxies: Clash.Proxy[]
  updatedAt?: string
  vehicleType: string
  subscriptionInfo?: {
    Upload?: number
    Download?: number
    Total?: number
    Expire?: number
  }
  testUrl?: string
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
