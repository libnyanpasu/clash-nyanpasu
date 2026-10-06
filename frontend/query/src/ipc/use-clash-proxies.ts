import { useCallback } from 'react'
import { unwrapResult } from '@nyanpasu/rpc'
import type {
  DelayHistory,
  Proxies_Serialize,
  Proxy_Serialize,
  ProxyGroup,
} from '@nyanpasu/rpc/types'
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryKey,
} from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, invokeQuery } from './query-options'

export type ClashDelayOptions = {
  url?: string
  timeout?: number
}

// Query data stays plain JSON: functions in it would defeat structural
// sharing, so every refetch would hand every group and node a new identity.
// Actions are returned by the hook instead.
export type ClashProxiesQueryProxyItem = Proxy_Serialize

export type ClashProxiesQueryGroupItem = ProxyGroup

export type ClashProxiesQuery = Proxies_Serialize

// Append a delay sample to a node's history, returning a new node object.
const withDelaySample = (
  node: ClashProxiesQueryProxyItem,
  delay: number,
): ClashProxiesQueryProxyItem => ({
  ...node,
  history: [
    ...node.history,
    { time: new Date().toISOString(), delay },
  ] satisfies DelayHistory[],
})

// How many members of a pinned group are tested at once.
const PINNED_GROUP_DELAY_CONCURRENCY = 8

// Runs `task` for every item, at most `limit` at a time.
const forEachConcurrently = async <T>(
  items: readonly T[],
  limit: number,
  task: (item: T) => Promise<void>,
) => {
  let next = 0
  const worker = async () => {
    while (next < items.length) {
      const item = items[next]
      next += 1
      await task(item)
    }
  }

  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, worker),
  )
}

export const useClashProxies = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const proxiesOptions = api.queries.getProxies()
  const selectProxyCommand = api.mutations.selectProxy
  const clearProxyFixedCommand = api.mutations.clearProxyFixed

  const { mutateAsync: mutateSelectProxy } = useMutation({
    mutationKey: selectProxyCommand.mutationKey,
    mutationFn: async ({ group, name }: { group: string; name: string }) =>
      unwrapResult(await invokeMutation(selectProxyCommand, [group, name])),
  })

  const { mutateAsync: mutateClearProxyFixed } = useMutation({
    mutationKey: clearProxyFixedCommand.mutationKey,
    mutationFn: async (group: string) =>
      unwrapResult(await invokeMutation(clearProxyFixedCommand, [group])),
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

  // Refetch through the client: reading any property of `proxies` during
  // render makes it a tracked property, and the query then re-renders this
  // hook only when a tracked property changes.
  const selectProxy = useCallback(
    async (group: string, name: string) => {
      await mutateSelectProxy({ group, name })
      await queryClient.refetchQueries({
        queryKey: api.queries.getProxies().queryKey,
        exact: true,
      })
    },
    [mutateSelectProxy, queryClient],
  )

  const clearProxyFixed = useCallback(
    async (group: string) => {
      await mutateClearProxyFixed(group)
      await queryClient.refetchQueries({
        queryKey: api.queries.getProxies().queryKey,
        exact: true,
      })
    },
    [mutateClearProxyFixed, queryClient],
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
          api.queries.clashApiGetProxyDelay(
            name,
            provider,
            options?.url ?? null,
            null,
          ),
        ),
      )
      return {
        name,
        delay: res?.delay ?? 0,
      }
    },
    onSuccess: ({ name, delay }) => {
      const oldData = getQueryData()
      const node = oldData?.nodes[name]

      if (!oldData || !node) {
        return
      }

      const newData = {
        ...oldData,
        nodes: { ...oldData.nodes, [name]: withDelaySample(node, delay) },
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
      const url = options?.url ?? null
      // The cached snapshot can predate a pin made in flight, in another
      // window, or by an external dashboard, so ask the core before choosing.
      const data = unwrapResult(
        await invokeMutation(api.mutations.mutateProxies, []),
      )
      const target =
        data.global?.name === group
          ? data.global
          : data.groups.find((item) => item.name === group)

      // Mihomo and Meow clear a pin before testing a group, so a pinned group
      // tests its members one by one instead.
      if (!target?.fixed) {
        return (
          unwrapResult(
            await invokeQuery(
              api.queries.clashApiGetGroupDelay(group, url, null),
            ),
          ) ?? {}
        )
      }

      const delays: Record<string, number> = {}
      await forEachConcurrently(
        target.all,
        PINNED_GROUP_DELAY_CONCURRENCY,
        async (name) => {
          try {
            const result = unwrapResult(
              await invokeQuery(
                api.queries.clashApiGetProxyDelay(
                  name,
                  data.nodes[name]?.provider ?? null,
                  url,
                  null,
                ),
              ),
            )
            delays[name] = result?.delay ?? 0
          } catch {
            // The core records a failed test as a zero sample; mirror it.
            delays[name] = 0
          }
        },
      )
      return delays
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

      const nodes = { ...oldData.nodes }
      for (const [name, delay] of Object.entries(data)) {
        const node = nodes[name]
        if (node) {
          nodes[name] = withDelaySample(node, delay)
        }
      }

      const newData = { ...oldData, nodes } satisfies ClashProxiesQuery

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
    clearProxyFixed,
    updateProxiesDelay,
    updateGroupDelay,
  }
}
