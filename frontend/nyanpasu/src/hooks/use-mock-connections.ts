import { useDebugMockNow, useDebugMockSetting } from './use-debug-mock'

// Dev builds only: the connections page shows generated connections instead
// of the core's.
export const useMockConnectionsSetting = () =>
  useDebugMockSetting('debug-mock-connections')

/** The time to generate mock connections for, ticking every second; null while off. */
export function useMockConnectionsNow(): number | null {
  const [enabled] = useMockConnectionsSetting()

  return useDebugMockNow(enabled)
}
