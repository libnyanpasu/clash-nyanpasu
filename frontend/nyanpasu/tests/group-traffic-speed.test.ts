import { expect, test } from 'vitest'
import type { ClashConnectionsSummary } from '../../interface/src/ipc/use-clash-connections'
import { latestGroupTrafficSpeed } from '../src/pages/(main)/main/proxies/_modules/group-traffic-speed.ts'

const summary = (
  memberRates: ClashConnectionsSummary['memberRates'],
): ClashConnectionsSummary => ({
  downloadTotal: 0,
  uploadTotal: 0,
  downloadSpeed: 0,
  uploadSpeed: 0,
  memberRates,
  memory: null,
  connections: null,
})

test('reads the latest sample only, and a group absent from it has no rate', () => {
  const snapshots = [
    summary({ group: { download: 999, upload: 999 } }),
    summary({ group: { download: 10, upload: 20 } }),
  ]
  expect(latestGroupTrafficSpeed(snapshots, 'group')).toEqual({
    download: 10,
    upload: 20,
  })
  expect(latestGroupTrafficSpeed(snapshots, 'other')).toEqual({
    download: 0,
    upload: 0,
  })
})

test('no samples yet reports zero', () => {
  expect(latestGroupTrafficSpeed([], 'group')).toEqual({
    download: 0,
    upload: 0,
  })
})
