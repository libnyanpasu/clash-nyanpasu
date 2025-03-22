import { useQuery } from '@tanstack/react-query'
import { useClashAPI, type ClashProviderRule } from '../service/clash-api'
import { CLASH_RULES_PROVIDER_QUERY_KEY } from './consts'

export interface ClashRulesProviderQueryItem extends ClashProviderRule {
  mutate: () => Promise<void>
}

export type ClashRulesProviderQuery = Record<
  string,
  ClashRulesProviderQueryItem
>

export const useClashRulesProvider = () => {
  const { providersRules, putProvidersRules } = useClashAPI()

  const query = useQuery({
    queryKey: [CLASH_RULES_PROVIDER_QUERY_KEY],
    queryFn: async () => {
      const { providers } = await providersRules()

      return Object.fromEntries(
        Object.entries(providers).map(([key, value]) => [
          key,
          {
            ...value,
            mutate: async () => {
              await putProvidersRules(key)
              await query.refetch()
            },
          },
        ]),
      ) satisfies ClashRulesProviderQuery
    },
  })

  return {
    ...query,
  }
}
