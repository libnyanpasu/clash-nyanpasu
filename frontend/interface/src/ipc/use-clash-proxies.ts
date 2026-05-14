import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import {
  commands,
  ProxyItemHistory,
  type Proxies_Serialize,
  type ProxyGroupItem_Serialize,
  type ProxyItem_Serialize,
} from './bindings'
import { CLASH_PROXIES_QUERY_KEY } from './consts'

export type ClashDelayOptions = {
  url?: string
  timeout?: number
}

export type ClashProxiesQueryHelperFn = {
  mutateDelay: (options?: ClashDelayOptions) => Promise<void>
}

export interface ClashProxiesQueryProxyItem
  extends ProxyItem_Serialize, ClashProxiesQueryHelperFn {
  mutateSelect: () => Promise<void>
}

export interface ClashProxiesQueryGroupItem
  extends ProxyGroupItem_Serialize, ClashProxiesQueryHelperFn {
  all: ClashProxiesQueryProxyItem[]
}

export interface ClashProxiesQuery extends Proxies_Serialize {
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

  const proxies = useQuery<ClashProxiesQuery | undefined>({
    queryKey: [CLASH_PROXIES_QUERY_KEY],
    queryFn: async () => {
      const result = unwrapResult(await commands.getProxies())

      if (!result) {
        return
      }

      // Create helper functions to reduce code duplication
      const createProxyWithHelpers = (
        proxy: ProxyItem_Serialize,
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
        group: ProxyGroupItem_Serialize,
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
    mutationFn: async (args: [string, ClashDelayOptions?]) => {
      const [name, options] = args
      const res = unwrapResult(
        await commands.clashApiGetProxyDelay(name, options?.url ?? null),
      )
      return {
        name,
        delay: res?.delay ?? 0,
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

  const updateGroupDelay = useMutation<
    Record<string, number>,
    unknown,
    [string, ClashDelayOptions?],
    ReturnType<typeof setInterval>
  >({
    mutationFn: async (args: [string, ClashDelayOptions?]) => {
      const [group, options] = args
      return (
        unwrapResult(
          await commands.clashApiGetGroupDelay(group, options?.url ?? null),
        ) ?? {}
      )
    },
    onMutate: () => {
      // Start polling proxies every 0.5 seconds
      const intervalId = setInterval(() => {
        proxies.refetch()
      }, 500)
      // Return interval ID to be used in onSettled
      return intervalId
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
    onSettled: (_, __, ___, context) => {
      // Clear interval when mutation is settled (success or error)
      if (context !== undefined) {
        clearInterval(context)
      }
    },
  })

  return {
    proxies,
    updateProxiesDelay,
    updateGroupDelay,
  }
}
