import { unwrapResult } from '@nyanpasu/rpc'
import { type RuleProviderItem } from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, invokeQuery } from './query-options'

export type ClashRulesProviderQueryItem = RuleProviderItem

export type ClashRulesProviderQuery = Record<
  string,
  ClashRulesProviderQueryItem
>

export const useClashRulesProvider = (
  options: { refetchInterval?: number | false; enabled?: boolean } = {},
) => {
  const api = useQueryApi()
  const providersQuery = api.queries.clashApiGetProvidersRules()
  return useQuery({
    ...options,
    queryKey: providersQuery.queryKey,
    queryFn: async (): Promise<ClashRulesProviderQuery> =>
      unwrapResult(await invokeQuery(providersQuery))?.providers ?? {},
  })
}

export const useUpdateClashRulesProvider = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const updateProvidersRules = api.mutations.clashApiUpdateProvidersRules
  return useMutation({
    mutationKey: updateProvidersRules.mutationKey,
    mutationFn: async (name: string) =>
      unwrapResult(await invokeMutation(updateProvidersRules, [name])),
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: api.queries.clashApiGetProvidersRules().queryKey,
      }),
  })
}
