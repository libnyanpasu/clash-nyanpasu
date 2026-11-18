import type {
  ClashCore_Deserialize,
  ClashCore_Serialize,
} from '@nyanpasu/rpc/types'
import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

/** Safe summary of the agent skills shipped by a core's release archive. */
export const useCoreAgentSkillsSummary = (
  core?: ClashCore_Serialize | null,
) => {
  const api = useQueryApi()
  const options = api.queries.getCoreAgentSkillsSummary(
    core as ClashCore_Deserialize,
  )

  return useQuery({
    ...unwrapQueryOptions(options, options.queryFn!),
    enabled: !!core,
  })
}
