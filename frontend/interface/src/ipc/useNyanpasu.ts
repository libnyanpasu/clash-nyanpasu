import { useMemo } from 'react'
import useSWR from 'swr'
import * as service from '@/service'
import { VergeConfig } from '@/service'
import { fetchCoreVersion, fetchLatestCore } from '@/service/core'
import * as tauri from '@/service/tauri'
import { useClash } from './useClash'

/**
 * useNyanpasu with swr.
 * Data from tauri backend.
 */
export const useNyanpasu = (options?: {
  onSuccess?: (data?: VergeConfig) => void
  onUpdate?: (data?: VergeConfig) => void
  onError?: (error: any) => void
  onLatestCoreError?: (error: any) => void
}) => {
  const { getConfigs, setConfigs, deleteConnections } = useClash()

  const { data, error, mutate } = useSWR<VergeConfig>(
    'nyanpasuConfig',
    service.getNyanpasuConfig,
    {
      onSuccess: options?.onSuccess,
    },
  )

  const setNyanpasuConfig = async (payload: Partial<VergeConfig>) => {
    try {
      await service.patchNyanpasuConfig(payload)

      const result = await mutate()

      if (options?.onUpdate) {
        options?.onUpdate(result)
      }
    } catch (error) {
      if (options?.onError) {
        options?.onError(error)
      } else {
        throw error
      }
    }
  }

  const getClashCore = useSWR('getClashCore', fetchCoreVersion)

  const setClashCore = async (
    clashCore: Required<VergeConfig>['clash_core'],
  ) => {
    await service.setClashCore(clashCore)

    // timeout for restart clash core.
    setTimeout(() => {
      getClashCore.mutate()
    }, 100)
  }

  const getLatestCore = useSWR('getLatestCore', fetchLatestCore, {
    revalidateOnMount: false,
    revalidateOnFocus: false,
    refreshInterval: 0,
    onError: options?.onLatestCoreError,
  })

  const updateCore = async (core: Required<VergeConfig>['clash_core']) => {
    return await service.updateCore(core)

    // getClashCore.mutate();
  }

  const getSystemProxy = useSWR('getSystemProxy', service.getSystemProxy)

  const getServiceStatus = useSWR('getServiceStatus', service.statusService)

  const setServiceStatus = async (
    type: 'install' | 'uninstall' | 'start' | 'stop',
  ) => {
    switch (type) {
      case 'install':
        await service.installService()
        break

      case 'uninstall':
        await service.uninstallService()
        break

      case 'start':
        await service.startService()
        break

      case 'stop':
        await service.stopService()
        break

      default:
        break
    }

    return getServiceStatus.mutate()
  }

  const setCurrentMode = async (mode: string) => {
    await deleteConnections()

    await setConfigs({ mode })

    await mutate()
  }

  const getCurrentMode = useMemo(() => {
    const modes: { [key: string]: boolean } = {
      rule: false,
      global: false,
      direct: false,
    }

    if (data?.clash_core === 'clash') {
      modes.script = false
    }

    const mode = getConfigs.data?.mode?.toLowerCase()

    if (mode && Object.prototype.hasOwnProperty.call(modes, mode)) {
      modes[mode] = true
    } else {
      modes.rule = true
    }

    return modes
  }, [data?.clash_core, getConfigs.data?.mode])

  return {
    nyanpasuConfig: data,
    isLoading: !data && !error,
    isError: error,
    setNyanpasuConfig,
    getCoreVersion: tauri.getCoreVersion,
    getClashCore,
    setClashCore,
    restartSidecar: tauri.restartSidecar,
    getLatestCore,
    updateCore,
    getSystemProxy,
    getServiceStatus,
    setServiceStatus,
    getCurrentMode,
    setCurrentMode,
  }
}
