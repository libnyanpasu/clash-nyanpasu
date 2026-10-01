import { m } from '@/paraglide/messages'
import type { Dimension } from '@nyanpasu/interface'

export type UsageLabel = {
  text: string
  /** The full value, for when the text is cut short. */
  title: string
  mono: boolean
}

const isIp = (value: string) =>
  /^\d{1,3}(\.\d{1,3}){3}$/.test(value) ||
  (value.includes(':') && /^[0-9a-f:.]+$/i.test(value))

/**
 * How a group's key reads on screen. `profiles` maps profile uids to names; it
 * is undefined until the profiles are loaded.
 */
export function usageLabel(
  dimension: Dimension,
  key: string,
  profiles?: ReadonlyMap<string, string>,
): UsageLabel {
  const plain = (text: string, title = text): UsageLabel => ({
    text,
    title,
    mono: false,
  })

  if (key === '') {
    return plain(
      dimension === 'chain' ? m.topology_no_group() : m.topology_unknown(),
    )
  }

  if (key === 'unknown') {
    return plain(m.topology_unknown())
  }

  switch (dimension) {
    case 'process':
      // The history keeps the whole path; the file name is what people know.
      return plain(key.split('/').pop() || key, key)
    case 'profile':
      return plain(
        profiles?.get(key) ??
          (profiles ? m.traffic_profile_deleted({ uid: key }) : key),
        key,
      )
    case 'origin':
    case 'source':
    case 'target':
      return { text: key, title: key, mono: isIp(key) }
    default:
      return plain(key)
  }
}

export const dimensionName = (dimension: Dimension) =>
  ({
    origin: m.traffic_dimension_origin,
    process: m.traffic_dimension_process,
    source: m.traffic_dimension_source,
    inbound: m.traffic_dimension_inbound,
    target: m.traffic_dimension_target,
    protocol: m.traffic_dimension_protocol,
    rule: m.traffic_dimension_rule,
    chain: m.traffic_dimension_chain,
    exit: m.traffic_dimension_exit,
    profile: m.traffic_dimension_profile,
    source_region: m.traffic_dimension_source_region,
    destination_region: m.traffic_dimension_destination_region,
  })[dimension]()
