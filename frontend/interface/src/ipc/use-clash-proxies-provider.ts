import { useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { invokeMutation, invokeQuery } from './query-options'
import { rpc } from './rpc'
import { type ProxyProviderItem_Serialize } from './rpc-bindings'

export interface ClashProxiesProviderQueryItem extends ProxyProviderItem_Serialize {
  mutate: () => Promise<void>
}

export type ClashProxiesProviderQuery = Record<
  string,
  ClashProxiesProviderQueryItem
>

export const useClashProxiesProvider = () => {
  const providersQuery = rpc.queries.clashApiGetProvidersProxies()
  const updateProxyProvider = rpc.mutations.updateProxyProvider
  const query = useQuery({
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
              ...value,
              mutate: async () => {
                unwrapResult(await invokeMutation(updateProxyProvider, [key]))
                await query.refetch()
              },
            },
          ]),
      ) as ClashProxiesProviderQuery
    },
  })

  return {
    ...query,
  }
}
