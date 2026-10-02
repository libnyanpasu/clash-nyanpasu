import type { ClashLog } from '@nyanpasu/query'
import type { LogRow } from '@nyanpasu/rpc/types'

const LEVELS = ['info', 'warning', 'error', 'debug']

// Core log lines as mihomo writes them: mostly one-line connection records,
// with an occasional long line that wraps over several rows.
export function createLogsFixture(count: number): ClashLog[] {
  return Array.from({ length: count }, (_, i) => {
    const target = `${['www', 'api', 'cdn', 'static'][i % 4]}.example-${i % 97}.com:443`
    const payload =
      i % 25 === 0
        ? `[TCP] dial Proxy (match RuleSet(geolocation-!cn)/Group ${i % 30}) 127.0.0.1:${50000 + i} --> ${target} error: ${'connect failed: dial tcp 203.0.113.7:443: i/o timeout; '.repeat(4)}`
        : `[TCP] 127.0.0.1:${50000 + i} --> ${target} match RuleSet(proxy) using Group ${i % 30}[🇯🇵 Node ${i % 1500}]`

    return {
      type: LEVELS[i % LEVELS.length],
      time: new Date(1_700_000_000_000 + i * 1000).toLocaleTimeString(),
      payload,
    }
  })
}

// One page of a nyanpasu log file as the file log viewer receives it.
export function createFileLogsFixture(count: number): LogRow[] {
  return Array.from({ length: count }, (_, i) => {
    const message =
      i % 25 === 0
        ? `failed to refresh subscription ${i % 13}: ${'error sending request: operation timed out; '.repeat(4)}`
        : `applied runtime config revision ${i} in ${(i * 7) % 300} ms`
    const timestamp = String(1_700_000_000_000 + i * 1000)

    return {
      id: String(i),
      timestamp,
      level: (['info', 'warn', 'error', 'debug'] as const)[i % 4],
      target: `nyanpasu::core::module_${i % 11}`,
      message,
      raw: JSON.stringify({ timestamp, message }),
      unparsed: false,
      truncated: false,
    }
  })
}
