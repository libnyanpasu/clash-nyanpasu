import { unwrapResult } from '@interface/utils'
import { useQuery } from '@tanstack/react-query'
import { commands } from './bindings'
import { SYSTEM_ACCENT_COLOR_QUERY_KEY } from './consts'

export const useSystemAccentColor = () => {
  const query = useQuery({
    queryKey: [SYSTEM_ACCENT_COLOR_QUERY_KEY],
    queryFn: async () => {
      return unwrapResult(await commands.getSystemAccentColor())
    },
    refetchInterval: 5000,
    refetchIntervalInBackground: true,
  })

  return {
    systemAccentColor: query.data,
    ...query,
  }
}
