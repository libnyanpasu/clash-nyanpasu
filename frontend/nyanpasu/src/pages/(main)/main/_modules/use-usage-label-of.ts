import { useCallback, useMemo } from 'react'
import { usageLabel } from '@/utils/traffic-usage'
import { useProfileListing } from '@nyanpasu/query'
import type { Dimension } from '@nyanpasu/rpc/types'

/** How a group's key reads on screen, with profile uids named once loaded. */
export function useUsageLabelOf() {
  const { data: profiles } = useProfileListing()

  const profileNames = useMemo(
    () =>
      profiles && new Map(profiles.items.map((item) => [item.uid, item.name])),
    [profiles],
  )

  return useCallback(
    (dimension: Dimension, key: string) =>
      usageLabel(dimension, key, profileNames),
    [profileNames],
  )
}
