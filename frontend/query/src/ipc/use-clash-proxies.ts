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

export const useClashProxies = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const proxiesOptions = api.queries.getProxies()
  const selectProxyCommand = api.mutations.selectProxy

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
      return (
        unwrapResult(
          await invokeQuery(
            api.queries.clashApiGetGroupDelay(group, options?.url ?? null),
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
    updateProxiesDelay,
    updateGroupDelay,
  }
}
