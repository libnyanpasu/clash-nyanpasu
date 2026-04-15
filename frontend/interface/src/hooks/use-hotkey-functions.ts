import { commands } from '@/ipc'
import { useQuery } from '@tanstack/react-query'

export const HOTKEY_FUNCTIONS_QUERY_KEY = 'hotkey_functions'

export function useHotkeyFunctions() {
  const query = useQuery({
    queryKey: [HOTKEY_FUNCTIONS_QUERY_KEY],
    queryFn: async () => {
      return await commands.getHotkeyFunctions()
    },
  })

  return query
}
