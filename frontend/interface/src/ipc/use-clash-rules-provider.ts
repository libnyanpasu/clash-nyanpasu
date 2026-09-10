import { useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { mutations, queries, type RuleProviderItem } from './bindings'
import { invokeMutation, invokeQuery } from './query-options'

export interface ClashRulesProviderQueryItem extends RuleProviderItem {
  mutate: () => Promise<void>
}

export type ClashRulesProviderQuery = Record<
  string,
  ClashRulesProviderQueryItem
>

export const useClashRulesProvider = () => {
  const providersQuery = queries.clashApiGetProvidersRules()
  const updateProvidersRules = mutations.clashApiUpdateProvidersRules
  const query = useQuery({
    queryKey: providersQuery.queryKey,
    queryFn: async () => {
      const result = unwrapResult(await invokeQuery(providersQuery))

      if (!result) return {}

      const { providers } = result

      return Object.fromEntries(
        Object.entries(providers).map(([key, value]) => [
          key,
          {
            ...value,
            mutate: async () => {
              unwrapResult(await invokeMutation(updateProvidersRules, [key]))
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
