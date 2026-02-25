import { useMemo } from 'react'
import {
  ClashProxiesQueryGroupItem,
  useClashConnections,
} from '@nyanpasu/interface'

export function useCurrentGroupConnection(
  currentGroup?: ClashProxiesQueryGroupItem,
) {
  const {
    query: { data: clashConnections },
  } = useClashConnections()

  return useMemo(() => {
    if (!currentGroup?.name) {
      return
    }

    return clashConnections
      ?.at(-1)
      ?.connections?.find((connection) =>
        connection.chains.includes(currentGroup?.name),
      )
  }, [clashConnections, currentGroup?.name])
}
