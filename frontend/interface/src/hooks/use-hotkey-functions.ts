import { queries } from '@interface/ipc'
import { useQuery } from '@tanstack/react-query'

export function useHotkeyFunctions() {
  const options = queries.getHotkeyFunctions()
  const query = useQuery(options)

  return query
}
