import { useMemo } from 'react'
import { unwrapResult } from '@nyanpasu/rpc'
import type { Mode } from '@nyanpasu/rpc/types'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation } from './query-options'
import { useClashConfig } from './use-clash-config'
import { useSettings } from './use-settings'

export type ProxyMode = 'rule' | 'global' | 'direct' | 'script'

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
 * - The backend applies the configured connection-interruption policy
 */
export const useProxyMode = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const clashConfig = useClashConfig()
  const setMode = api.mutations.setProxyMode

  const upsertMode = useMutation({
    mutationKey: setMode.mutationKey,
    mutationFn: async (mode: Mode) => {
      return unwrapResult(await invokeMutation(setMode, [mode]))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: api.queries.clashApiGetSettings().queryKey,
      })
      queryClient.invalidateQueries({
        queryKey: api.queries.clashApiGetConfigs().queryKey,
      })
    },
  })

  const settings = useSettings()
  const clashCore = settings.query.data?.core

  const value = useMemo(() => {
    const modes: Record<'rule' | 'global' | 'direct', boolean> & {
      script?: boolean
    } = {
      rule: false,
      global: false,
      direct: false,
    }

    // only clash premium support script mode
    if (clashCore === 'clash') {
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
  }, [clashConfig.query.data, clashCore])

  const upsert = async (mode: ProxyMode) => {
    // only clash premium support script mode
    if (clashCore !== 'clash' && mode === 'script') {
      throw new Error('Script mode is only available for Clash Premium')
    }

    return await upsertMode.mutateAsync(mode)
  }

  return {
    value,
    query: clashConfig.query,
    settingsQuery: settings.query,
    isPending: upsertMode.isPending,
    upsert,
  }
}
