import { unwrapResult } from '@nyanpasu/rpc'
import type {
  ClashApiConfig,
  ClashGuardOverridesPatch_Deserialize,
} from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, unwrapQueryOptions } from './query-options'

export const useClashConfig = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const configQuery = api.queries.clashApiGetConfigs()
  const patchConfig = api.mutations.patchRuntimeOverrides

  const query = useQuery<ClashApiConfig | undefined>(
    unwrapQueryOptions(configQuery, configQuery.queryFn!),
  )

  const upsert = useMutation({
    mutationKey: patchConfig.mutationKey,
    mutationFn: async (payload: ClashGuardOverridesPatch_Deserialize) => {
      return unwrapResult(await invokeMutation(patchConfig, [payload]))
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
