import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { invokeMutation, unwrapQueryOptions } from './query-options'
import { rpc } from './rpc'
import { type ClashConfig, type PatchRuntimeConfig } from './rpc-bindings'

export const useClashConfig = () => {
  const queryClient = useQueryClient()
  const configQuery = rpc.queries.clashApiGetConfigs()
  const patchConfig = rpc.mutations.patchClashConfig

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
