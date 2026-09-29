import { useQuery } from '@tanstack/react-query'
import { unwrapQueryOptions } from './query-options'
import { rpc } from './rpc'

export const useSystemAccentColor = () => {
  const query = useQuery({
    ...unwrapQueryOptions(
      rpc.queries.getSystemAccentColor(),
      rpc.queries.getSystemAccentColor().queryFn!,
    ),
    refetchInterval: 5000,
    refetchIntervalInBackground: true,
  })

  return {
    systemAccentColor: query.data,
    ...query,
  }
}
