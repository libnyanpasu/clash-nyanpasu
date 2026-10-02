import type { ClashConnectionsSummary } from '@nyanpasu/rpc/types'

export type GroupTrafficSpeed = {
  download: number
  upload: number
}

// The latest sample's per-chain-member rate table already carries this
// group's rate (Rust sums it over every connection whose `chains` contains
// the group's name); no diffing needed here any more.
export function latestGroupTrafficSpeed(
  snapshots: ClashConnectionsSummary[],
  groupName: string,
): GroupTrafficSpeed {
  const rate = snapshots.at(-1)?.memberRates[groupName]
  return rate
    ? { download: rate.download, upload: rate.upload }
    : { download: 0, upload: 0 }
}
