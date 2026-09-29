import { useQuery } from '@tanstack/react-query'
import { unwrapQueryOptions } from './query-options'
import { rpc } from './rpc'

export const useServerPort = () => {
  const { data: serverPort } = useQuery(
    unwrapQueryOptions(
      rpc.queries.getServerPort(),
      rpc.queries.getServerPort().queryFn!,
    ),
  )

  return serverPort
}
