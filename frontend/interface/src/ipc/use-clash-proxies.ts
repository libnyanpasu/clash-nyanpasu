import { useCallback } from 'react'
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryKey,
} from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import {
  mutations,
  queries,
  type Proxies_Serialize,
  type ProxyGroupItem_Serialize,
  type ProxyItem_Serialize,
  type ProxyItemHistory,
} from './bindings'
import { invokeMutation, invokeQuery } from './query-options'

export type ClashDelayOptions = {
  url?: string
  timeout?: number
}

// Query data stays plain JSON: functions in it would defeat structural
// sharing, so every refetch would hand every group and node a new identity.
// Actions are returned by the hook instead.
export type ClashProxiesQueryProxyItem = ProxyItem_Serialize

export type ClashProxiesQueryGroupItem = ProxyGroupItem_Serialize

export type ClashProxiesQuery = Proxies_Serialize

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
  const proxiesOptions = queries.getProxies()
  const selectProxyCommand = mutations.selectProxy

  const { mutateAsync: mutateSelectProxy } = useMutation({
    mutationKey: selectProxyCommand.mutationKey,
    mutationFn: async ({ group, name }: { group: string; name: string }) =>
      unwrapResult(await invokeMutation(selectProxyCommand, [group, name])),
  })

  const proxies = useQuery<ClashProxiesQuery | undefined>({
    queryKey: proxiesOptions.queryKey,
    queryFn: async () => {
      const result = unwrapResult(await invokeQuery(proxiesOptions))

      if (!result) {
        return
      }

      return {
        ...result,
        groups: result.groups.filter((group) => !group.hidden),
      } satisfies ClashProxiesQuery
    },
  })

  const { refetch: refetchProxies } = proxies

  const selectProxy = useCallback(
    async (group: string, name: string) => {
      await mutateSelectProxy({ group, name })
      await refetchProxies()
    },
    [mutateSelectProxy, refetchProxies],
  )

  const getQueryData = () => {
    return queryClient.getQueryData(proxiesOptions.queryKey) as
      ClashProxiesQuery | undefined
  }

  const setQueryData = (data: ClashProxiesQuery) => {
    queryClient.setQueryData<ClashProxiesQuery | undefined>(
      proxiesOptions.queryKey as QueryKey,
      data,
    )
  }

  const updateProxiesDelay = useMutation({
    mutationFn: async (args: [string, string | null, ClashDelayOptions?]) => {
      const [name, provider, options] = args
      const res = unwrapResult(
        await invokeQuery(
          queries.clashApiGetProxyDelay(name, provider, options?.url ?? null),
        ),
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
          await invokeQuery(
            queries.clashApiGetGroupDelay(group, options?.url ?? null),
          ),
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
              : proxy,
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
              : proxy,
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
    selectProxy,
    updateProxiesDelay,
    updateGroupDelay,
  }
}
