import type {
  ClashConnection_Serialize,
  ClashConnectionDetails_Serialize,
  ClashRule,
  UsageGroup,
} from '@nyanpasu/rpc/types'

const RULE_TYPES = ['DomainSuffix', 'DomainKeyword', 'Domain', 'IPCIDR']

const GROUPS = Array.from({ length: 20 }, (_, i) => `Group ${i}`)

// A rule set as a large subscription has: mostly domain rules spread over a
// few groups, with the final MATCH.
export function createRulesFixture(count: number): ClashRule[] {
  return Array.from({ length: count }, (_, i) =>
    i === count - 1
      ? { type: 'Match', payload: '', proxy: GROUPS[0] }
      : {
          type: RULE_TYPES[i % RULE_TYPES.length],
          payload: `site-${i}.example.com`,
          proxy: GROUPS[i % GROUPS.length],
        },
  )
}

// Session totals the traffic store answers for the rules it has seen.
export function createUsageFixture(keys: readonly string[]): UsageGroup[] {
  return keys
    .filter((_, i) => i % 3 === 0)
    .map((key, i) => ({
      key,
      usage: {
        bytes: { download: 1024 * (i + 1) * 997, upload: 1024 * (i + 1) * 13 },
        connections: i + 1,
      },
      current_rate: null,
    }))
}

// Every frame replaces 1 in this many connections with a new one.
const CHURN = 100

// The connection-detail frames the core streams once a second. Each frame is
// freshly deserialized, so every connection object is new; a few connections
// close while as many open. Most connections go idle after their first
// seconds, as keep-alive connections do, so their traffic stops changing.
export function createConnectionStream(count: number, rules: ClashRule[]) {
  // Connections match the first rules, as real traffic does.
  const matched = rules.slice(0, Math.min(rules.length, 200))
  const base = Date.UTC(2026, 9, 1)

  return (frame: number): ClashConnectionDetails_Serialize => ({
    sequence: frame,
    connections: Array.from({ length: count }, (_, slot) => {
      const generation = Math.floor((frame + slot) / CHURN)
      const seed = slot * 7919 + generation * 104729
      const elapsed = (frame + slot) % CHURN
      const rate = 1024 * ((seed % 4096) + 1)
      const rule = matched[seed % matched.length]
      const busySeconds = seed % 10 < 7 ? Math.min(elapsed, 2) : elapsed
      const wave = busySeconds === elapsed ? 1 + Math.sin(frame / 3 + slot) : 0

      return {
        id: `conn-${slot}-${generation}`,
        upload: Math.round((rate / 20) * busySeconds),
        download: rate * busySeconds,
        uploadSpeed: Math.round((rate / 20) * wave),
        downloadSpeed: Math.round(rate * wave),
        start: new Date(base + (frame - elapsed) * 1000).toISOString(),
        chains: [`Node ${seed % 300}`, GROUPS[seed % GROUPS.length], 'Proxy'],
        rule: rule.type,
        rulePayload: rule.payload,
        metadata: {
          network: seed % 7 === 0 ? 'udp' : 'tcp',
          type: 'Tun',
          host: rule.payload || `host-${seed % 5000}.example.net`,
          sourceIP: '198.18.0.1',
          sourcePort: String(40000 + (seed % 20000)),
          destinationIP: `203.0.${seed % 256}.${(seed >> 8) % 256}`,
          destinationPort: '443',
          inboundName: 'DEFAULT-TUN',
          process: `app-${seed % 40}.exe`,
          processPath: `C:\\Program Files\\App ${seed % 40}\\app-${seed % 40}.exe`,
          dnsMode: 'fake-ip',
          specialProxy: '',
          specialRules: '',
          remoteDestination: '',
          _extra: { dscp: 0, sniffHost: '' },
        },
        providerChains: [],
        _extra: {},
      } satisfies ClashConnection_Serialize
    }),
  })
}
