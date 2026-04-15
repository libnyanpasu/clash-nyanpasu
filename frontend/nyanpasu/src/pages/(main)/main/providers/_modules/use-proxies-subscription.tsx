import { useMemo } from 'react'
import { ClashProviderProxies } from '@nyanpasu/interface'

const clampPercentage = (value: number) => Math.min(100, Math.max(0, value))

export const useProxiesSubscription = (data: ClashProviderProxies) => {
  return useMemo(() => {
    let progress = 0
    let total = 0
    let used = 0

    const hasSubscriptionInfo =
      'subscriptionInfo' in data && data.subscriptionInfo !== undefined

    if (hasSubscriptionInfo) {
      const subscriptionInfo = data.subscriptionInfo as Record<
        string,
        number | undefined
      >

      const download =
        subscriptionInfo.download ?? subscriptionInfo.Download ?? 0
      const upload = subscriptionInfo.upload ?? subscriptionInfo.Upload ?? 0
      const t = subscriptionInfo.total ?? subscriptionInfo.Total ?? 0

      total = t

      used = download + upload

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
