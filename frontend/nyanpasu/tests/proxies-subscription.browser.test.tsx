import { expect, test } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { useProxiesSubscription } from '@/pages/(main)/main/providers/_modules/use-proxies-subscription'
import type { ClashProxiesProviderQueryItem } from '@nyanpasu/query'

const provider = (
  subscriptionInfo: ClashProxiesProviderQueryItem['subscriptionInfo'],
): ClashProxiesProviderQueryItem => ({
  name: 'sub',
  type: 'Proxy',
  vehicleType: 'HTTP',
  updatedAt: null,
  subscriptionInfo,
  proxyCount: 1,
})

test('a provider without a subscription reports none instead of throwing', async () => {
  const { result } = await renderHook(() =>
    useProxiesSubscription(provider(null)),
  )

  expect(result.current).toEqual({
    progress: 0,
    total: 0,
    used: 0,
    hasSubscriptionInfo: false,
  })
})

test('usage sums upload and download against the total', async () => {
  const { result } = await renderHook(() =>
    useProxiesSubscription(
      provider({ Upload: 10, Download: 30, Total: 200, Expire: 0 }),
    ),
  )

  expect(result.current).toEqual({
    progress: 20,
    total: 200,
    used: 40,
    hasSubscriptionInfo: true,
  })
})

test('negative counters never produce negative usage', async () => {
  const { result } = await renderHook(() =>
    useProxiesSubscription(
      provider({ Upload: -10, Download: 30, Total: -1, Expire: 0 }),
    ),
  )

  expect(result.current).toEqual({
    progress: 0,
    total: 0,
    used: 30,
    hasSubscriptionInfo: true,
  })
})

test('a negative download never reduces the used amount', async () => {
  const { result } = await renderHook(() =>
    useProxiesSubscription(
      provider({ Upload: 30, Download: -10, Total: 100, Expire: 0 }),
    ),
  )

  expect(result.current).toEqual({
    progress: 30,
    total: 100,
    used: 30,
    hasSubscriptionInfo: true,
  })
})
