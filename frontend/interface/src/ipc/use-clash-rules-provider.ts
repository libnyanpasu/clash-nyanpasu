import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { invokeMutation, invokeQuery } from './query-options'
import { rpc } from './rpc'
import { type RuleProviderItem } from './rpc-bindings'

export type ClashRulesProviderQueryItem = RuleProviderItem

export type ClashRulesProviderQuery = Record<
  string,
  ClashRulesProviderQueryItem
>

export const useClashRulesProvider = () => {
  const providersQuery = rpc.queries.clashApiGetProvidersRules()
  return useQuery({
    queryKey: providersQuery.queryKey,
    queryFn: async (): Promise<ClashRulesProviderQuery> =>
      unwrapResult(await invokeQuery(providersQuery))?.providers ?? {},
  })
}

export const useUpdateClashRulesProvider = () => {
  const queryClient = useQueryClient()
  const updateProvidersRules = rpc.mutations.clashApiUpdateProvidersRules
  return useMutation({
    mutationKey: updateProvidersRules.mutationKey,
    mutationFn: async (name: string) =>
      unwrapResult(await invokeMutation(updateProvidersRules, [name])),
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: rpc.queries.clashApiGetProvidersRules().queryKey,
      }),
  })
}
