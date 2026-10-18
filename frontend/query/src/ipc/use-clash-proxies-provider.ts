import { unwrapResult } from '@nyanpasu/rpc'
import { type ProxyProviderSummary_Serialize } from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, invokeQuery } from './query-options'

export type ClashProxiesProviderQueryItem = ProxyProviderSummary_Serialize

export type ClashProxiesProviderQuery = Record<
  string,
  ClashProxiesProviderQueryItem
>

export const useClashProxiesProvider = (
  options: { refetchInterval?: number | false; enabled?: boolean } = {},
) => {
  const api = useQueryApi()
  const providersQuery = api.queries.clashApiGetProvidersProxies()
  return useQuery({
    ...options,
    queryKey: providersQuery.queryKey,
    queryFn: async () => {
      const result = unwrapResult(await invokeQuery(providersQuery))

      if (!result) return {} as ClashProxiesProviderQuery

      return Object.fromEntries(
        Object.entries(result).filter(([, value]) =>
          ['http', 'file'].includes(value.vehicleType.toLowerCase()),
        ),
      )
    },
  })
}

export const useUpdateClashProxiesProvider = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const updateProxyProvider = api.mutations.updateProxyProvider
  return useMutation({
    mutationKey: updateProxyProvider.mutationKey,
    mutationFn: async (name: string) =>
      unwrapResult(await invokeMutation(updateProxyProvider, [name])),
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: api.queries.clashApiGetProvidersProxies().queryKey,
      }),
  })
}
