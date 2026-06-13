import { useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands, type RuleProviderItem } from './bindings'
import { CLASH_RULES_PROVIDER_QUERY_KEY } from './consts'

export interface ClashRulesProviderQueryItem extends RuleProviderItem {
  mutate: () => Promise<void>
}

export type ClashRulesProviderQuery = Record<
  string,
  ClashRulesProviderQueryItem
>

export const useClashRulesProvider = () => {
  const query = useQuery({
    queryKey: [CLASH_RULES_PROVIDER_QUERY_KEY],
    queryFn: async () => {
      const result = unwrapResult(await commands.clashApiGetProvidersRules())

      if (!result) return {}

      const { providers } = result

      return Object.fromEntries(
        Object.entries(providers).map(([key, value]) => [
          key,
          {
            ...value,
            mutate: async () => {
              unwrapResult(await commands.clashApiUpdateProvidersRules(key))
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
