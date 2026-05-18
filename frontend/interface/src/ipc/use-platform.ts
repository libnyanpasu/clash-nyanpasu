import { unwrapResult } from '@interface/utils'
import { useQuery } from '@tanstack/react-query'
import { commands } from './bindings'
import { IS_APPIMAGE_QUERY_KEY } from './consts'

export const useIsAppImage = () => {
  return useQuery({
    queryKey: [IS_APPIMAGE_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.isAppimage()),
    staleTime: Infinity,
  })
}
