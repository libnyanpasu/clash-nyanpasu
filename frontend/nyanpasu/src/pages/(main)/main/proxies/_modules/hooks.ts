import { useMemo } from 'react'
import { useClashConnections } from '@nyanpasu/interface'
import { latestGroupTrafficSpeed } from './group-traffic-speed'

export function useGroupTrafficSpeed(groupName?: string) {
  const { data: clashConnections } = useClashConnections()

  return useMemo(
    () =>
      groupName
        ? latestGroupTrafficSpeed(clashConnections, groupName)
        : { download: 0, upload: 0 },
    [clashConnections, groupName],
  )
}
