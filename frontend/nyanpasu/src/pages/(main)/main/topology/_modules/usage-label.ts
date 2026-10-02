import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import parseTraffic from '@/utils/parse-traffic'
import type { Dimension, Metric, Usage } from '@nyanpasu/rpc/types'

export type UsageLabel = {
  text: string
  /** The full value, for when the text is cut short. */
  title: string
  mono: boolean
}

let regionNames: { locale: string; names: Intl.DisplayNames } | undefined

/** The localized name of a region code, `unknown` or whatever else a key is. */
export function regionName(code: string) {
  if (code === 'unknown') {
    return m.topology_unknown()
  }

  // `DisplayNames.of` throws for anything but a well-formed region code.
  if (!/^(?:[a-z]{2}|\d{3})$/i.test(code)) {
    return code
  }

  const locale = getLocale()

  if (regionNames?.locale !== locale) {
    regionNames = {
      locale,
      names: new Intl.DisplayNames([locale], { type: 'region' }),
    }
  }

  try {
    return regionNames.names.of(code) ?? code
  } catch {
    return code
  }
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
    case 'source_region':
    case 'destination_region':
      return plain(regionName(key), key)
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

/** What the metric weighs a usage by. */
export const usageValue = ({ bytes, connections }: Usage, metric: Metric) =>
  metric === 'bytes' ? bytes.upload + bytes.download : connections

export const usageAmount = (usage: Usage, metric: Metric) =>
  metric === 'bytes'
    ? parseTraffic(usageValue(usage, metric)).join(' ')
    : m.topology_connection_count({ count: usage.connections })

/**
 * The largest of the values, or `floor` when none is larger. A loop: spreading
 * a whole graph into `Math.max` overflows the arguments of the call.
 */
export function largest(values: Iterable<number>, floor: number) {
  let result = floor

  for (const value of values) {
    if (value > result) result = value
  }

  return result
}
