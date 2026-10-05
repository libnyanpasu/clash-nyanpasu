import { useMemo } from 'react'
import { ClashProxiesProviderQueryItem } from '@nyanpasu/query'

const clampPercentage = (value: number) => Math.min(100, Math.max(0, value))

export const useProxiesSubscription = (data: ClashProxiesProviderQueryItem) => {
  return useMemo(() => {
    let progress = 0
    let total = 0
    let used = 0

    // A provider without a subscription header reports null usage.
    const subscriptionInfo = data.subscriptionInfo
    const hasSubscriptionInfo = subscriptionInfo != null

    if (hasSubscriptionInfo) {
      total = subscriptionInfo.Total

      used = subscriptionInfo.Download + subscriptionInfo.Upload

      if (total > 0) {
        progress = clampPercentage((used / total) * 100)
      }
    }

    return {
      progress,
      total,
      used,
      hasSubscriptionInfo,
    }
  }, [data])
}
