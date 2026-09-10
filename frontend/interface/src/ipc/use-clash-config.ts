import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import {
  mutations,
  queries,
  type ClashConfig,
  type PatchRuntimeConfig,
} from './bindings'
import { invokeMutation, unwrapQueryOptions } from './query-options'

export const useClashConfig = () => {
  const queryClient = useQueryClient()
  const configQuery = queries.clashApiGetConfigs()
  const patchConfig = mutations.patchClashConfig

  const query = useQuery<ClashConfig | undefined>(
    unwrapQueryOptions(configQuery, configQuery.queryFn!),
  )

  const upsert = useMutation({
    mutationKey: patchConfig.mutationKey,
    mutationFn: async (payload: PatchRuntimeConfig & Partial<ClashConfig>) => {
      return unwrapResult(
        await invokeMutation(patchConfig, [payload as PatchRuntimeConfig]),
      )
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: configQuery.queryKey,
      })
    },
  })

  return {
    query,
    upsert,
  }
}
