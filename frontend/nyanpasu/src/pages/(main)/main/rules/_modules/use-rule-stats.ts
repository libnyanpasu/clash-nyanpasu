import { useDeferredValue, useMemo } from 'react'
import {
  useClashConnectionDetails,
  useCurrentProfileUid,
  useTrafficUsageByKeys,
} from '@nyanpasu/query'
import {
  type Bytes,
  type ClashRule,
  type TrafficQuery,
} from '@nyanpasu/rpc/types'

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

// A rule belongs to the profile whose config defined it, so its totals only
// count the connections of the current profile; the rest of the recorded
// history does not apply to it.
const totalsQuery = (profile: string): TrafficQuery => ({
  range: 'all',
  scope: 'all',
  filters: [{ dimension: 'profile', value: profile }],
})

const TOTALS_REFRESH_INTERVAL = 10_000

/**
 * Live stats of the current connections and recorded traffic totals of the
 * current profile, keyed by `ruleLabel`. `totals` is undefined while traffic
 * recording is unavailable.
 */
export function useRuleStats(rules: readonly ClashRule[]) {
  const { data: latest } = useClashConnectionDetails()

  // Every sample re-sorts the rules by their live stats; a deferred sample
  // renders in the background, where input and scrolling interrupt it.
  const details = useDeferredValue(latest)

  const profile = useCurrentProfileUid()

  // No selected profile is the empty profile key.
  const query = useMemo(() => totalsQuery(profile ?? ''), [profile])

  const labels = useMemo(
    () => [...new Set(rules.map((rule) => ruleLabel(rule.type, rule.payload)))],
    [rules],
  )

  const { data: usage } = useTrafficUsageByKeys(query, 'rule', labels, {
    refetchInterval: TOTALS_REFRESH_INTERVAL,
  })

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
      new Map<string, Bytes>(
        usage.map((group) => [group.key, group.usage.bytes]),
      ),
    [usage],
  )

  return { live, totals }
}
