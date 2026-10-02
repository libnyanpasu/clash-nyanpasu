import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

export const useServerPort = () => {
  const api = useQueryApi()
  const { data: serverPort } = useQuery(
    unwrapQueryOptions(
      api.queries.getServerPort(),
      api.queries.getServerPort().queryFn!,
    ),
  )

  return serverPort
}
