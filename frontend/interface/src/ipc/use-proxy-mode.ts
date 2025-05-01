import { useMemo } from 'react'
import { useClashAPI } from '../service/clash-api'
import { useClashConfig } from './use-clash-config'
import { useSetting } from './use-settings'

/**
 * Hook for managing proxy mode in Clash configuration
 *
 * @returns {Object} An object containing:
 * - value: Record of available proxy modes (rule, global, direct, script) with their active states
 * - upsert: Function to update the proxy mode and delete existing connections
 *
 * @remarks
 * - Script mode is only available when using Clash Premium
 * - Default mode is 'rule' if current mode is invalid or not set
 * - Changes to proxy mode will clear all existing connections
 */
export const useProxyMode = () => {
  const clashConfig = useClashConfig()

  const clashCore = useSetting('clash_core')

  const { deleteConnections } = useClashAPI()

  const value = useMemo(() => {
    const modes: Record<'rule' | 'global' | 'direct', boolean> & {
      script?: boolean
    } = {
      rule: false,
      global: false,
      direct: false,
    }

    // only clash premium support script mode
    if (clashCore.value === 'clash') {
      modes.script = false
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

  const upsert = async (mode: string) => {
    await deleteConnections()

    await clashConfig.upsert.mutateAsync({ mode })
  }

  return {
    value,
    upsert,
  }
}
