import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import {
  mutations,
  queries,
  type ClashApiConfig,
  type ClashGuardOverridesPatch,
} from './bindings'
import { invokeMutation, unwrapQueryOptions } from './query-options'

export const useClashConfig = () => {
  const queryClient = useQueryClient()
  const configQuery = queries.clashApiGetConfigs()
  const patchConfig = mutations.patchRuntimeOverrides

  const query = useQuery<ClashApiConfig | undefined>(
    unwrapQueryOptions(configQuery, configQuery.queryFn!),
  )

  const upsert = useMutation({
    mutationKey: patchConfig.mutationKey,
    mutationFn: async (
      payload: ClashGuardOverridesPatch & Partial<ClashApiConfig>,
    ) => {
      return unwrapResult(
        await invokeMutation(patchConfig, [
          payload as ClashGuardOverridesPatch,
        ]),
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
