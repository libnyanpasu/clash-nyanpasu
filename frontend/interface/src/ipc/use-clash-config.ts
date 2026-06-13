import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands, type ClashConfig, type PatchRuntimeConfig } from './bindings'
import { CLASH_CONFIG_QUERY_KEY } from './consts'

export const useClashConfig = () => {
  const queryClient = useQueryClient()

  const query = useQuery<ClashConfig | undefined>({
    queryKey: [CLASH_CONFIG_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.clashApiGetConfigs()),
  })

  const upsert = useMutation({
    mutationFn: async (payload: PatchRuntimeConfig & Partial<ClashConfig>) => {
      return unwrapResult(
        await commands.patchClashConfig(payload as PatchRuntimeConfig),
      )
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [CLASH_CONFIG_QUERY_KEY] })
    },
  })

  return {
    query,
    upsert,
  }
}
