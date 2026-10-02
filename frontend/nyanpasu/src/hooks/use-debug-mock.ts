import { useEffect, useState } from 'react'
import { useLocalStorage } from '@uidotdev/usehooks'

// Dev builds only: a debug setting under `storageKey` that makes a page show
// generated data instead of the core's, to preview it without traffic.
export function useDebugMockSetting(storageKey: string) {
  // No initial value, so reading the setting never writes it.
  const [enabled, setEnabled] = useLocalStorage<boolean | undefined>(
    storageKey,
    undefined,
  )

  return [import.meta.env.DEV && enabled === true, setEnabled] as const
}

/**
 * The time to generate mock data for, ticking every second while `ticking`;
 * null while `enabled` is off.
 */
export function useDebugMockNow(
  enabled: boolean,
  ticking = true,
): number | null {
  const [now, setNow] = useState(Date.now)

  useEffect(() => {
    if (!enabled || !ticking) {
      return
    }

    const timer = setInterval(() => setNow(Date.now()), 1000)

    return () => clearInterval(timer)
  }, [enabled, ticking])

  return enabled ? now : null
}
