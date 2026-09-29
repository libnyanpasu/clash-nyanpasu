import { useMemo } from 'react'
import { ClashConnection, useClashConnections } from '@nyanpasu/interface'

export type GroupTrafficSpeed = {
  download: number
  upload: number
}

// Sums the per-connection byte delta between the latest two samples for the
// connections routed through the group; a connection absent from the previous
// sample has no rate yet, matching the connections page.
export function sumGroupTrafficSpeed(
  snapshots: ClashConnection[],
  groupName: string,
): GroupTrafficSpeed {
  const speed = { download: 0, upload: 0 }

  const latest = (snapshots.at(-1)?.connections ?? []).filter((connection) =>
    connection.chains.includes(groupName),
  )

  if (latest.length === 0) {
    return speed
  }

  const previous = new Map(
    (snapshots.at(-2)?.connections ?? []).map((connection) => [
      connection.id,
      connection,
    ]),
  )

  for (const connection of latest) {
    const prev = previous.get(connection.id)

    if (prev) {
      speed.download += connection.download - prev.download
      speed.upload += connection.upload - prev.upload
    }
  }

  return speed
}

export function useGroupTrafficSpeed(groupName?: string) {
  const { data: clashConnections } = useClashConnections()

  return useMemo(
    () =>
      groupName
        ? sumGroupTrafficSpeed(clashConnections, groupName)
        : { download: 0, upload: 0 },
    [clashConnections, groupName],
  )
}
