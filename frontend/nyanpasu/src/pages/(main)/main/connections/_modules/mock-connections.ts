import type {
  ClashConnection_Serialize,
  ClosedConnection,
} from '@nyanpasu/interface'

// Generated connections for previewing the connections page in dev builds.
// Each slot reconnects on its own period, so which connections are open or
// closed follows from the time alone: both tabs agree without shared state.

const SLOT_COUNT = 48

// How far back the closed tab reaches.
const CLOSED_WINDOW = 30 * 60 * 1000

const HOSTS = [
  'www.google.com',
  'github.com',
  'api.github.com',
  'raw.githubusercontent.com',
  'www.youtube.com',
  'i.ytimg.com',
  'registry.npmjs.org',
  'cdn.jsdelivr.net',
  'discord.com',
  'gateway.discord.gg',
  'api.telegram.org',
  'www.bilibili.com',
  'update.microsoft.com',
  'time.apple.com',
  'crates.io',
  'static.rust-lang.org',
]

const PROCESSES: ReadonlyArray<readonly [string, string]> = [
  [
    'Google Chrome',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  ],
  ['firefox', '/Applications/Firefox.app/Contents/MacOS/firefox'],
  ['node', '/usr/local/bin/node'],
  ['curl', '/usr/bin/curl'],
  ['git', '/usr/bin/git'],
  ['Telegram', '/Applications/Telegram.app/Contents/MacOS/Telegram'],
  ['Discord', '/Applications/Discord.app/Contents/MacOS/Discord'],
  ['cargo', '/Users/user/.cargo/bin/cargo'],
]

// Outermost group first; the wire order is the reverse.
const CHAINS: ReadonlyArray<readonly string[]> = [
  ['Proxy', 'Auto', 'HK 01'],
  ['Proxy', 'Auto', 'JP 02'],
  ['Proxy', 'US 03'],
  ['Streaming', 'SG 01'],
  ['DIRECT'],
]

const RULES: ReadonlyArray<readonly [string, string]> = [
  ['DomainSuffix', 'google.com'],
  ['DomainKeyword', 'github'],
  ['RuleSet', 'streaming'],
  ['GeoIP', 'CN'],
  ['Match', ''],
]

const TYPES = ['HTTP', 'HTTPS', 'Socks5', 'Tun']

type Slot = {
  index: number
  period: number
  lifetime: number
  phase: number
  downloadRate: number
  uploadRate: number
  host: string
  destinationIP: string
  process: readonly [string, string]
  chains: string[]
  rule: readonly [string, string]
  network: 'tcp' | 'udp'
  type: string
  sourceIP: string
}

// mulberry32: a small seeded generator, so every slot is the same on every run.
function seeded(seed: number) {
  let state = seed >>> 0

  return () => {
    state = (state + 0x6d2b79f5) >>> 0
    let t = state
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

function slot(index: number): Slot {
  const next = seeded(index + 1)
  const pick = <T>(items: readonly T[]) =>
    items[Math.floor(next() * items.length)]
  const byte = () => Math.floor(next() * 256)

  const period = (20 + next() * 580) * 1000
  const downloadRate = Math.round(1024 * 2 ** (next() * 12))

  return {
    index,
    period,
    lifetime: period * (0.3 + next() * 0.65),
    phase: next() * period,
    downloadRate,
    uploadRate: Math.round(downloadRate * (0.02 + next() * 0.3)),
    host: pick(HOSTS),
    destinationIP: `${pick([104, 140, 142, 151, 172])}.${byte()}.${byte()}.${byte()}`,
    process: pick(PROCESSES),
    chains: [...pick(CHAINS)].reverse(),
    rule: pick(RULES),
    network: next() < 0.85 ? 'tcp' : 'udp',
    type: pick(TYPES),
    sourceIP: `192.168.1.${2 + Math.floor(next() * 250)}`,
  }
}

const SLOTS = Array.from({ length: SLOT_COUNT }, (_, index) => slot(index))

function generationAt({ period, phase }: Slot, time: number) {
  return Math.floor((time + phase) / period)
}

function lifespan({ period, phase, lifetime }: Slot, generation: number) {
  const start = generation * period - phase

  return { start, end: start + lifetime }
}

const connectionId = ({ index }: Slot, generation: number) =>
  `mock-${index}-${generation}`

export function mockActiveConnections(
  now: number,
): ClashConnection_Serialize[] {
  return SLOTS.flatMap((slot) => {
    const generation = generationAt(slot, now)
    const { start, end } = lifespan(slot, generation)

    if (now >= end) {
      return []
    }

    const elapsed = (now - start) / 1000
    const wave = 0.55 + 0.45 * Math.sin(now / 2500 + slot.index)

    return [
      {
        id: connectionId(slot, generation),
        upload: Math.round(slot.uploadRate * elapsed),
        download: Math.round(slot.downloadRate * elapsed),
        uploadSpeed: Math.round(slot.uploadRate * wave),
        downloadSpeed: Math.round(slot.downloadRate * wave),
        start: new Date(start).toISOString(),
        chains: slot.chains,
        rule: slot.rule[0],
        rulePayload: slot.rule[1],
        metadata: {
          network: slot.network,
          type: slot.type,
          host: slot.host,
          sourceIP: slot.sourceIP,
          sourcePort: String(40000 + ((generation * 7919) % 20000)),
          destinationIP: slot.destinationIP,
          destinationPort: '443',
          process: slot.process[0],
          processPath: slot.process[1],
          _extra: {},
        },
        _extra: {},
      },
    ]
  })
}

export function mockClosedConnections(now: number): ClosedConnection[] {
  const closed = SLOTS.flatMap((slot) => {
    const connections: ClosedConnection[] = []

    for (let generation = generationAt(slot, now); ; generation--) {
      const { start, end } = lifespan(slot, generation)

      if (end > now) {
        continue
      }

      if (end <= now - CLOSED_WINDOW) {
        break
      }

      const seconds = (end - start) / 1000

      connections.push({
        id: connectionId(slot, generation),
        started_at: Math.round(start),
        first_seen_at: Math.round(start),
        closed_at: Math.round(end),
        bytes: {
          upload: Math.round(slot.uploadRate * seconds),
          download: Math.round(slot.downloadRate * seconds),
        },
        dimensions: {
          process: slot.process[1],
          source: slot.sourceIP,
          target: slot.host,
          protocol: slot.network,
          rule: { kind: slot.rule[0], payload: slot.rule[1] },
          chains: slot.chains,
        },
      })
    }

    return connections
  })

  // Newest first, like the traffic store.
  return closed.sort(
    (a, b) => b.closed_at - a.closed_at || b.id.localeCompare(a.id),
  )
}
