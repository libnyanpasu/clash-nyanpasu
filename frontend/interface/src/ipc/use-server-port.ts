import { useQuery } from '@tanstack/react-query'
import { queries } from './bindings'
import { unwrapQueryOptions } from './query-options'

export const useServerPort = () => {
  const { data: serverPort } = useQuery(
    unwrapQueryOptions(
      queries.getServerPort(),
      queries.getServerPort().queryFn!,
    ),
  )

  return serverPort
}
