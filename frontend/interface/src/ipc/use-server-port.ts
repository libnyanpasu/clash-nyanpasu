import { unwrapResult } from '@interface/utils'
import { useQuery } from '@tanstack/react-query'
import { commands } from './bindings'
import { SERVER_PORT_QUERY_KEY } from './consts'

export const useServerPort = () => {
  const { data: serverPort } = useQuery({
    queryKey: [SERVER_PORT_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.getServerPort()),
  })

  return serverPort
}
