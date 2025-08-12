import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useClashAPI, type ClashDelayOptions } from '../service/clash-api'
import { unwrapResult } from '../utils'
import {
  commands,
  ProxyItemHistory,
  type Proxies,
  type ProxyGroupItem,
  type ProxyItem,
} from './bindings'
import { CLASH_PROXIES_QUERY_KEY } from './consts'

export type ClashProxiesQueryHelperFn = {
  mutateDelay: (options?: ClashDelayOptions) => Promise<void>
}

export interface ClashProxiesQueryProxyItem
  extends ProxyItem,
    ClashProxiesQueryHelperFn {
  mutateSelect: () => Promise<void>
}

export interface ClashProxiesQueryGroupItem
  extends ProxyGroupItem,
    ClashProxiesQueryHelperFn {
  all: ClashProxiesQueryProxyItem[]
}

export interface ClashProxiesQuery extends Proxies {
  global: ClashProxiesQueryGroupItem
  groups: ClashProxiesQueryGroupItem[]
}

// Create a new proxy item with updated history
const createUpdatedProxy = (
  proxy: ClashProxiesQueryProxyItem,
  { name, delay }: { name: string; delay: number },
) => {
  if (proxy.name !== name) return proxy

  const newHistory = [
    ...proxy.history,
    { time: new Date().toISOString(), delay },
  ] satisfies ProxyItemHistory[]

  return { ...proxy, history: newHistory }
}

export const useClashProxies = () => {
  const queryClient = useQueryClient()

  const { proxiesDelay, groupDelay } = useClashAPI()

  const proxies = useQuery<ClashProxiesQuery | undefined>({
    queryKey: [CLASH_PROXIES_QUERY_KEY],
    queryFn: async () => {
      const result = unwrapResult(await commands.getProxies())

      if (!result) {
        return
      }

      // Create helper functions to reduce code duplication
      const createProxyWithHelpers = (
        proxy: ProxyItem,
        groupName: string,
      ): ClashProxiesQueryProxyItem => ({
        ...proxy,
        mutateDelay: async (options?: ClashDelayOptions) => {
          await updateProxiesDelay.mutateAsync([proxy.name, options])
        },
        mutateSelect: async () => {
          await commands.selectProxy(groupName, proxy.name)
          await proxies.refetch()
        },
      })

      const createGroupWithHelpers = (
        group: ProxyGroupItem,
      ): ClashProxiesQueryGroupItem => ({
        ...group,
        mutateDelay: async (options?: ClashDelayOptions) => {
          await updateGroupDelay.mutateAsync([group.name, options])
        },
        all: group.all.map((proxy) =>
          createProxyWithHelpers(proxy, group.name),
        ),
      })

      // Apply helper functions to groups and global
      const groups = result.groups
        .filter((g) => !g.hidden)
        .map(createGroupWithHelpers)
      const global = createGroupWithHelpers(result.global)

      // merge the results & type validation
      const merged = {
        ...result,
        groups,
        global,
      } satisfies ClashProxiesQuery

      return merged
    },
  })

  const getQueryData = () => {
    return queryClient.getQueryData([CLASH_PROXIES_QUERY_KEY]) as
      | ClashProxiesQuery
      | undefined
  }

  const setQueryData = (data: ClashProxiesQuery) => {
    queryClient.setQueryData([CLASH_PROXIES_QUERY_KEY], data)
  }

  const updateProxiesDelay = useMutation({
    mutationFn: async (args: Parameters<typeof proxiesDelay>) => {
      return {
        name: args[0],
        delay: (await proxiesDelay(...args)).delay,
      }
    },
    onSuccess: ({ name, delay }) => {
      const oldData = getQueryData()

      if (!oldData) {
        return
      }

      // Create new data structure with updated proxies
      const newData = {
        ...oldData,
        global: {
          ...oldData.global,
          all: oldData.global.all.map((proxy) =>
            createUpdatedProxy(proxy, { name, delay }),
          ),
        },
        groups: oldData.groups.map((group) => ({
          ...group,
          all: group.all.map((proxy) =>
            createUpdatedProxy(proxy, { name, delay }),
          ),
        })),
      } satisfies ClashProxiesQuery

      setQueryData(newData)
    },
  })

  const updateGroupDelay = useMutation({
    mutationFn: async (args: Parameters<typeof groupDelay>) => {
      return await groupDelay(...args)
    },
    onSuccess: (data) => {
      const oldData = getQueryData()

      if (!oldData) {
        return
      }

      // Create new data structure with updated proxies
      const newData = {
        ...oldData,
        global: {
          ...oldData.global,
          all: oldData.global.all.map((proxy) =>
            Object.prototype.hasOwnProperty.call(data, proxy.name)
              ? createUpdatedProxy(proxy, {
                  name: proxy.name,
                  delay: data[proxy.name],
                })
              : {
                  ...proxy,
                  history: [],
                },
          ),
        },
        groups: oldData.groups.map((group) => ({
          ...group,
          all: group.all.map((proxy) =>
            Object.prototype.hasOwnProperty.call(data, proxy.name)
              ? createUpdatedProxy(proxy, {
                  name: proxy.name,
                  delay: data[proxy.name],
                })
              : {
                  ...proxy,
                  history: [],
                },
          ),
        })),
      } satisfies ClashProxiesQuery

      setQueryData(newData)
    },
  })

  return {
    ...proxies,
  }
}
