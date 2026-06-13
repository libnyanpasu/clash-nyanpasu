import { useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands } from './bindings'
import { CLASH_RULES_QUERY_KEY } from './consts'

export const useClashRules = () => {
  const query = useQuery({
    queryKey: [CLASH_RULES_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.clashApiGetRules()),
  })

  return {
    ...query,
  }
}
