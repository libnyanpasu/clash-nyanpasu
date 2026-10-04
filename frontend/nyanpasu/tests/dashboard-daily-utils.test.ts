import { expect, test } from 'vitest'
import {
  calculateSubscriptionQuota,
  getRemoteFileConfigs,
  isExpiryWarning,
  isQuotaWarning,
  latestFinishedRuns,
  resolveProfileTarget,
  subscriptionExpiryTimestamp,
} from '@/components/widgets/dashboard-daily-utils'
import type {
  ProfileItem_Serialize,
  ProfileSubscriptionInfo,
  RunDto,
} from '@nyanpasu/rpc/types'

const remoteConfig = (
  uid: string,
  subscription?: ProfileSubscriptionInfo,
): ProfileItem_Serialize =>
  ({
    uid,
    name: uid,
    type: 'config',
    config: {
      type: 'file',
      source: {
        type: 'remote',
        url: `https://${uid}.example.test/config.yaml`,
        file: `profiles/${uid}.yaml`,
        option: {
          with_proxy: false,
          self_proxy: false,
          update_interval_minutes: 0,
        },
        subscription,
      },
    },
  }) as ProfileItem_Serialize

const composition = (uid: string): ProfileItem_Serialize =>
  ({
    uid,
    name: uid,
    type: 'config',
    config: {
      type: 'composition',
      base: 'source-a',
      extend_proxies_from: ['source-b'],
    },
  }) as ProfileItem_Serialize

const transform = (uid: string): ProfileItem_Serialize =>
  ({
    uid,
    name: uid,
    type: 'transform',
    transform: {
      source: {
        type: 'remote',
        url: `https://${uid}.example.test/transform.js`,
        file: `profiles/${uid}.js`,
        option: {
          with_proxy: false,
          self_proxy: false,
          update_interval_minutes: 0,
        },
      },
    },
  }) as ProfileItem_Serialize

test('profile targets follow a remote file config and never turn a composition into one', () => {
  const items = [remoteConfig('source-a'), composition('composed')]

  expect(resolveProfileTarget(items, 'source-a', { kind: 'current' })).toEqual({
    kind: 'selected',
    profile: items[0],
  })
  expect(resolveProfileTarget(items, 'composed', { kind: 'current' })).toEqual({
    kind: 'needs_selection',
  })
  expect(
    resolveProfileTarget(items, 'composed', {
      kind: 'fixed',
      profileUid: 'source-a',
    }),
  ).toEqual({ kind: 'selected', profile: items[0] })
  expect(
    resolveProfileTarget(items, 'source-a', {
      kind: 'fixed',
      profileUid: 'deleted',
    }),
  ).toEqual({ kind: 'missing' })
  expect(resolveProfileTarget([], null, { kind: 'current' })).toEqual({
    kind: 'needs_selection',
  })
})

test('quota candidates include only remote file configs and preserve explicit scope', () => {
  const items = [
    remoteConfig('remote-a'),
    remoteConfig('remote-b'),
    composition('composition'),
    transform('transform'),
  ]

  expect(getRemoteFileConfigs(items).map((item) => item.uid)).toEqual([
    'remote-a',
    'remote-b',
  ])
  expect(
    getRemoteFileConfigs(items, ['remote-b']).map((item) => item.uid),
  ).toEqual(['remote-b'])
  expect(
    resolveProfileTarget(items, 'transform', { kind: 'current' }).kind,
  ).toBe('unsupported')
})

test('subscription quota requires both byte directions and a positive total', () => {
  expect(
    calculateSubscriptionQuota({ upload: 25, download: 75, total: 200 }),
  ).toEqual({
    upload: 25,
    download: 75,
    used: 100,
    total: 200,
    remaining: 100,
    usedPercent: 50,
    remainingPercent: 50,
    overage: 0,
  })
  expect(
    calculateSubscriptionQuota({ upload: 101, download: 99, total: 100 }),
  ).toMatchObject({
    remaining: 0,
    usedPercent: 100,
    remainingPercent: 0,
    overage: 100,
  })
  expect(calculateSubscriptionQuota({ upload: 5, total: 20 })).toBeNull()
  expect(calculateSubscriptionQuota({ upload: 5, download: 5 })).toBeNull()
  expect(
    calculateSubscriptionQuota({ upload: 5, download: 5, total: 0 }),
  ).toBeNull()
  expect(
    calculateSubscriptionQuota({ upload: Number.NaN, download: 1, total: 2 }),
  ).toBeNull()
})

test('expiry and threshold helpers keep invalid timestamps unknown', () => {
  const timestamp = subscriptionExpiryTimestamp(1_800_000_000)
  if (timestamp === null) throw new Error('expected a valid expiry timestamp')
  expect(timestamp).toBe(1_800_000_000_000)
  expect(subscriptionExpiryTimestamp(0)).toBeNull()
  expect(subscriptionExpiryTimestamp(-1)).toBeNull()
  expect(subscriptionExpiryTimestamp(Number.POSITIVE_INFINITY)).toBeNull()
  expect(
    isExpiryWarning(timestamp, 7, timestamp - 2 * 24 * 60 * 60 * 1000),
  ).toBe(true)
  expect(
    isExpiryWarning(timestamp, 7, timestamp - 10 * 24 * 60 * 60 * 1000),
  ).toBe(false)
  expect(isExpiryWarning(timestamp, 7, timestamp + 1)).toBe(true)
  expect(
    isQuotaWarning(
      calculateSubscriptionQuota({ upload: 90, download: 0, total: 100 }),
      20,
    ),
  ).toBe(true)
  expect(isQuotaWarning(null, 20)).toBe(false)
})

const run = (
  id: string,
  admissionSequence: string,
  state: RunDto['state'],
  finishedAt: string | null,
) =>
  ({
    id,
    job: 'profile-sync',
    definition_version: '1',
    admission_sequence: admissionSequence,
    trigger: 'scheduled',
    scheduled_at: null,
    admitted_at: finishedAt ?? '2026-01-01T00:00:00Z',
    finished_at: finishedAt,
    state,
    last_log_sequence: '0',
    dropped_log_count: '0',
  }) as RunDto

test('recent sync results include only finished runs and sort newest first', () => {
  const finished = { kind: 'finished', completion: {} } as RunDto['state']
  const runs = [
    run('running', '4', { kind: 'running' }, null),
    run('old', '1', finished, '2026-01-01T00:00:00Z'),
    run('newest', '3', finished, '2026-01-03T00:00:00Z'),
    run('middle', '2', finished, '2026-01-02T00:00:00Z'),
  ]

  expect(latestFinishedRuns(runs, 2).map((item) => item.id)).toEqual([
    'newest',
    'middle',
  ])
})
