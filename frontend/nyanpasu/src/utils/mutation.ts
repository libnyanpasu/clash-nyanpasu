import { useCallback } from 'react'
import { cache, mutate } from 'swr/_internal'

export const useGlobalMutation = () => {
  return useCallback((swrKey, ...args) => {
    const matcher = typeof swrKey === 'function' ? swrKey : undefined

    if (matcher) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const keys = Array.from(cache.keys()).filter(matcher as any)
      keys.forEach((key) => mutate(key, ...args))
    } else {
      mutate(swrKey, ...args)
    }
  }, []) as typeof mutate
}
