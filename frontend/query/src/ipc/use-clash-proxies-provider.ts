import { unwrapResult } from '@nyanpasu/rpc'
import { type ProxyProvider_Serialize } from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, invokeQuery } from './query-options'

/**
 * The fields the providers pages show. The full items carry every node with
 * its delay history, which changes on every health check.
 */
export type ClashProxiesProviderQueryItem = Pick<
  ProxyProvider_Serialize,
  'name' | 'type' | 'vehicleType' | 'updatedAt' | 'subscriptionInfo'
> & {
  proxyCount: number
}

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
        Object.entries(result)
          .filter(([, value]) =>
            ['http', 'file'].includes(value.vehicleType.toLowerCase()),
          )
          .map(([key, value]) => [
            key,
            {
              name: value.name,
              type: value.type,
              vehicleType: value.vehicleType,
              updatedAt: value.updatedAt,
              subscriptionInfo: value.subscriptionInfo,
              proxyCount: value.proxies.length,
            },
          ]),
      ) as ClashProxiesProviderQuery
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
