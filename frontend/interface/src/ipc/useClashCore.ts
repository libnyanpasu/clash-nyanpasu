import useSWR from 'swr'
import {
  Clash,
  clash as clashApi,
  ProviderItem,
  ProviderRules,
} from '@/service'
import * as tauri from '@/service/tauri'

export const useClashCore = () => {
  const { getGroupDelay, getProxiesDelay, ...clash } = clashApi()

  const { data, isLoading, mutate } = useSWR('getProxies', tauri.getProxies)

  const updateGroupDelay = async (
    index: number,
    options?: Clash.DelayOptions,
  ) => {
    const group = data?.groups[index]

    if (!group) {
      return
    }

    await getGroupDelay(group?.name, options)

    await mutate(() => tauri.mutateProxies())
  }

  const updateProxiesDelay = async (
    name: string,
    options?: Clash.DelayOptions,
  ) => {
    const result = await getProxiesDelay(name, options)

    await mutate()

    return result
  }

  const setGroupProxy = async (index: number, name: string) => {
    const group = data?.groups[index]

    if (!group) {
      return
    }

    await tauri.selectProxy(group?.name, name)

    await mutate()
  }

  const setGlobalProxy = async (name: string) => {
    const group = data?.global

    if (!group) {
      return
    }

    await tauri.selectProxy(group?.name, name)

    await mutate()
  }

  const getRules = useSWR('getRules', clash.getRules)

  const getRulesProviders = useSWR<{ [name: string]: ProviderRules }>(
    'getRulesProviders',
    clash.getRulesProviders,
  )

  const updateRulesProviders = async (name: string) => {
    await clash.updateRulesProviders(name)

    await getRulesProviders.mutate()
  }

  const getProxiesProviders = useSWR<{ [name: string]: ProviderItem }>(
    'getProxiesProviders',
    clash.getProxiesProviders,
  )

  const getAllProxiesProviders = useSWR<{ [name: string]: ProviderItem }>(
    'getAllProxiesProviders',
    clash.getAllProxiesProviders,
  )

  const updateProxiesProviders = async (name: string) => {
    await clash.updateProxiesProviders(name)

    await getProxiesProviders.mutate()
  }

  return {
    data,
    isLoading,
    updateGroupDelay,
    updateProxiesDelay,
    setGroupProxy,
    setGlobalProxy,
    getRules,
    getRulesProviders,
    updateRulesProviders,
    getProxiesProviders,
    getAllProxiesProviders,
    updateProxiesProviders,
  }
}
