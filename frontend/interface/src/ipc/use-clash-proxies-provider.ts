import { useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import {
  mutations,
  queries,
  type ProxyProviderItem_Serialize,
} from './bindings'
import { invokeMutation, invokeQuery } from './query-options'

export interface ClashProxiesProviderQueryItem extends ProxyProviderItem_Serialize {
  mutate: () => Promise<void>
}

export type ClashProxiesProviderQuery = Record<
  string,
  ClashProxiesProviderQueryItem
>

export const useClashProxiesProvider = () => {
  const providersQuery = queries.clashApiGetProvidersProxies()
  const updateProxyProvider = mutations.updateProxyProvider
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
