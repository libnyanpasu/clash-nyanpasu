import type { ClashConnectionItem } from '@nyanpasu/interface'

export type GeographicRegion = {
  code: string
  count: number
  bytes: number
}
export type GeographicRoute = GeographicRegion & {
  source: string
  destination: string
}

// Only an unambiguous region reported by the core can be placed on the map.
// Multiple GeoIP tags must not duplicate a connection's bytes across countries.
export function connectionRegion(
  connection: ClashConnectionItem,
  endpoint: 'source' | 'destination',
  knownRegions: ReadonlySet<string>,
): string {
  const codes =
    connection.metadata?.[
      endpoint === 'source' ? 'sourceGeoIP' : 'destinationGeoIP'
    ]
  const normalized = [
    ...new Set((codes ?? []).map((code) => code.toUpperCase())),
  ]
  return normalized.length === 1 && knownRegions.has(normalized[0])
    ? normalized[0]
    : 'unknown'
}

export function buildGeography(
  connections: ClashConnectionItem[],
  knownRegions: ReadonlySet<string>,
) {
  const regions = new Map<string, GeographicRegion>()
  const routes = new Map<string, GeographicRoute>()
  for (const connection of connections) {
    const destination = connectionRegion(
      connection,
      'destination',
      knownRegions,
    )
    const source = connectionRegion(connection, 'source', knownRegions)
    const bytes =
      Math.max(0, connection.upload) + Math.max(0, connection.download)
    const region = regions.get(destination) ?? {
      code: destination,
      count: 0,
      bytes: 0,
    }
    region.count++
    region.bytes += bytes
    regions.set(destination, region)
    if (
      source === 'unknown' ||
      destination === 'unknown' ||
      source === destination
    )
      continue
    const key = `${source}:${destination}`
    const route = routes.get(key) ?? {
      code: key,
      source,
      destination,
      count: 0,
      bytes: 0,
    }
    route.count++
    route.bytes += bytes
    routes.set(key, route)
  }
  return { regions: [...regions.values()], routes: [...routes.values()] }
}
