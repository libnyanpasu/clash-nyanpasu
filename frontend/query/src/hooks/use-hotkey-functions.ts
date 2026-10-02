import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

export function useHotkeyFunctions() {
  const api = useQueryApi()
  const options = api.queries.getHotkeyFunctions()
  const query = useQuery(options)

  return query
}
