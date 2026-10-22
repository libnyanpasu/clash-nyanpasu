import type {
  ClashCore_Deserialize,
  ClashCore_Serialize,
} from '@nyanpasu/rpc/types'
import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

/** The agent skills a core's release archive ships, which the core updater keeps. */
export const useCoreAgentSkills = (core?: ClashCore_Serialize | null) => {
  const api = useQueryApi()
  const options = api.queries.getCoreAgentSkills(core as ClashCore_Deserialize)

  return useQuery({
    ...unwrapQueryOptions(options, options.queryFn!),
    enabled: !!core,
  })
}
