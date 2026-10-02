import { useDebugMockNow, useDebugMockSetting } from './use-debug-mock'

// Dev builds only: the traffic page shows generated usage instead of the
// recorded traffic.
export const useMockTrafficSetting = () =>
  useDebugMockSetting('debug-mock-traffic')

/**
 * The time to generate mock traffic for, ticking every second unless
 * `paused`; null while off.
 */
export function useMockTrafficNow(paused = false): number | null {
  const [enabled] = useMockTrafficSetting()

  return useDebugMockNow(enabled, !paused)
}
