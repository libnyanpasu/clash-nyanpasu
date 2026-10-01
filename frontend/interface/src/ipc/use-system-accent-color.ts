import { useQuery } from '@tanstack/react-query'
import { unwrapQueryOptions } from './query-options'
import { rpc } from './rpc'

export const useSystemAccentColor = () => {
  const query = useQuery({
    ...unwrapQueryOptions(
      rpc.queries.getSystemAccentColor(),
      rpc.queries.getSystemAccentColor().queryFn!,
    ),
    // Polled while the window is visible; only the color is exposed so a
    // poll that finds nothing new does not re-render the caller.
    refetchInterval: 5000,
  })

  return { systemAccentColor: query.data }
}
