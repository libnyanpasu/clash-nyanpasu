import { useQuery } from '@tanstack/react-query'
import { useClashAPI, type ClashProviderProxies } from '../service/clash-api'
import { CLASH_PROXIES_PROVIDER_QUERY_KEY } from './consts'

export interface ClashProxiesProviderQueryItem extends ClashProviderProxies {
  mutate: () => Promise<void>
}

export type ClashProxiesProviderQuery = Record<
  string,
  ClashProxiesProviderQueryItem
>

export const useClashProxiesProvider = () => {
  const { providersProxies, putProvidersProxies } = useClashAPI()

  const query = useQuery({
    queryKey: [CLASH_PROXIES_PROVIDER_QUERY_KEY],
    queryFn: async () => {
      const { providers } = await providersProxies()

      return Object.fromEntries(
        Object.entries(providers).map(([key, value]) => [
          key,
          {
            ...value,
            mutate: async () => {
              await putProvidersProxies(key)
              await query.refetch()
            },
          },
        ]),
      )
    },
  })

  return {
    ...query,
  }
}
