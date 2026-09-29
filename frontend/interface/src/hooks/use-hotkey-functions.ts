import { rpc } from '@interface/ipc'
import { useQuery } from '@tanstack/react-query'

export function useHotkeyFunctions() {
  const options = rpc.queries.getHotkeyFunctions()
  const query = useQuery(options)

  return query
}
