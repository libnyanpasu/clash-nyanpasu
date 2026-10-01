import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { invokeMutation, invokeQuery } from './query-options'
import { rpc } from './rpc'
import { type ProxyProviderItem_Serialize } from './rpc-bindings'

/**
 * The fields the providers pages show. The full items carry every node with
 * its delay history, which changes on every health check.
 */
export type ClashProxiesProviderQueryItem = Pick<
  ProxyProviderItem_Serialize,
  'name' | 'type' | 'vehicleType' | 'updatedAt' | 'subscriptionInfo'
> & {
  proxyCount: number
}

export type ClashProxiesProviderQuery = Record<
  string,
  ClashProxiesProviderQueryItem
>

export const useClashProxiesProvider = () => {
  const providersQuery = rpc.queries.clashApiGetProvidersProxies()
  return useQuery({
    queryKey: providersQuery.queryKey,
    queryFn: async () => {
      const result = unwrapResult(await invokeQuery(providersQuery))

      if (!result) return {} as ClashProxiesProviderQuery

      const { providers } = result

      return Object.fromEntries(
        Object.entries(providers)
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
  const queryClient = useQueryClient()
  const updateProxyProvider = rpc.mutations.updateProxyProvider
  return useMutation({
    mutationKey: updateProxyProvider.mutationKey,
    mutationFn: async (name: string) =>
      unwrapResult(await invokeMutation(updateProxyProvider, [name])),
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: rpc.queries.clashApiGetProvidersProxies().queryKey,
      }),
  })
}
