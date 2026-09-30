import { useMemo } from 'react'
import {
  useClashConnectionDetails,
  useTrafficUsageByKeys,
  type Bytes,
  type ClashRule,
} from '@nyanpasu/interface'

/**
 * Identifies a rule the way a connection's `rule` / `rulePayload` and the
 * traffic store's rule key (`RuleKey::label`) do.
 */
export const ruleLabel = (type: string, payload: string) =>
  payload ? `${type},${payload}` : type

export type RuleLiveStats = {
  connections: number
  downloadSpeed: number
  uploadSpeed: number
}

/**
 * Live stats of the current connections and session traffic totals, keyed by
 * `ruleLabel`. `totals` is undefined while traffic recording is unavailable.
 */
export function useRuleStats(rules: readonly ClashRule[]) {
  const { data: details } = useClashConnectionDetails()

  const labels = useMemo(
    () => [...new Set(rules.map((rule) => ruleLabel(rule.type, rule.payload)))],
    [rules],
  )

  const { data: usage } = useTrafficUsageByKeys('rule', labels)

  const live = useMemo(() => {
    const stats = new Map<string, RuleLiveStats>()

    for (const conn of details?.connections ?? []) {
      const label = ruleLabel(conn.rule, conn.rulePayload)
      const slot = stats.get(label)

      if (slot) {
        slot.connections += 1
        slot.downloadSpeed += conn.downloadSpeed
        slot.uploadSpeed += conn.uploadSpeed
      } else {
        stats.set(label, {
          connections: 1,
          downloadSpeed: conn.downloadSpeed,
          uploadSpeed: conn.uploadSpeed,
        })
      }
    }

    return stats
  }, [details])

  const totals = useMemo(
    () =>
      usage &&
      new Map<string, Bytes>(usage.map((group) => [group.key, group.bytes])),
    [usage],
  )

  return { live, totals }
}
