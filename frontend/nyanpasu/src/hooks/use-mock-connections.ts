import { useEffect, useState } from 'react'
import { useLocalStorage } from '@uidotdev/usehooks'

const STORAGE_KEY = 'debug-mock-connections'

// Dev builds only: a debug setting that makes the connections page show
// generated connections instead of the core's, to preview it without traffic.
export function useMockConnectionsSetting() {
  // No initial value, so reading the setting never writes it.
  const [enabled, setEnabled] = useLocalStorage<boolean | undefined>(
    STORAGE_KEY,
    undefined,
  )

  return [import.meta.env.DEV && enabled === true, setEnabled] as const
}

/** The time to generate mock connections for, ticking every second; null while off. */
export function useMockConnectionsNow(): number | null {
  const [enabled] = useMockConnectionsSetting()

  const [now, setNow] = useState(Date.now)

  useEffect(() => {
    if (!enabled) {
      return
    }

    const timer = setInterval(() => setNow(Date.now()), 1000)

    return () => clearInterval(timer)
  }, [enabled])

  return enabled ? now : null
}
