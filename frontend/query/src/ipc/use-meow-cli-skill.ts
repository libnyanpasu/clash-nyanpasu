import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

/** The meow-cli agent skill the core updater keeps from Meow archives. */
export const useMeowCliSkill = () => {
  const api = useQueryApi()
  const query = useQuery(
    unwrapQueryOptions(
      api.queries.getMeowCliSkill(),
      api.queries.getMeowCliSkill().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
