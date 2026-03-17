import { useMemo } from 'react'
import { ClashProviderProxies } from '@nyanpasu/interface'

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

      progress = (used / (total || 1)) * 100
    }

    return {
      progress,
      total,
      used,
      hasSubscriptionInfo,
    }
  }, [data])
}
