import { useMemo } from 'react'
import { useClashConfig } from './use-clash-config'

export type ProxyMode = 'rule' | 'global' | 'direct'

/**
 * Hook for managing proxy mode in Clash configuration
 *
 * @returns {Object} An object containing:
 * - value: Record of available proxy modes (rule, global, direct) with their active states
 * - upsert: Function to update the proxy mode and delete existing connections
 *
 * @remarks
 * - Default mode is 'rule' if current mode is invalid or not set
 * - The backend applies the configured connection-interruption policy
 */
export const useProxyMode = () => {
  const clashConfig = useClashConfig()

  const value = useMemo(() => {
    const modes: Record<ProxyMode, boolean> = {
      rule: false,
      global: false,
      direct: false,
    }

    const currentMode = clashConfig.query.data?.mode?.toLowerCase()

    if (
      currentMode &&
      Object.prototype.hasOwnProperty.call(modes, currentMode)
    ) {
      modes[currentMode as keyof typeof modes] = true
    } else {
      modes.rule = true
    }

    return modes
  }, [clashConfig.query.data])

  const upsert = async (mode: ProxyMode) => {
    return await clashConfig.upsert.mutateAsync({ mode })
  }

  return {
    value,
    query: clashConfig.query,
    isPending: clashConfig.upsert.isPending,
    upsert,
  }
}
