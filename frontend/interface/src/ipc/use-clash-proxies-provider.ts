import { useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands, type ProxyProviderItem_Serialize } from './bindings'
import { CLASH_PROXIES_PROVIDER_QUERY_KEY } from './consts'

export interface ClashProxiesProviderQueryItem extends ProxyProviderItem_Serialize {
  mutate: () => Promise<void>
}

export type ClashProxiesProviderQuery = Record<
  string,
  ClashProxiesProviderQueryItem
>

export const useClashProxiesProvider = () => {
  const query = useQuery({
    queryKey: [CLASH_PROXIES_PROVIDER_QUERY_KEY],
    queryFn: async () => {
      const result = unwrapResult(await commands.clashApiGetProvidersProxies())

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
                unwrapResult(await commands.updateProxyProvider(key))
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
