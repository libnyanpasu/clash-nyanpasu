import { useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands } from './bindings'
import { CLASH_VERSION_QUERY_KEY } from './consts'

export const useClashVersion = () => {
  const query = useQuery({
    queryKey: [CLASH_VERSION_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.clashApiGetVersion()),
  })

  return query
}
