import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

export const useSystemAccentColor = () => {
  const api = useQueryApi()
  const query = useQuery({
    ...unwrapQueryOptions(
      api.queries.getSystemAccentColor(),
      api.queries.getSystemAccentColor().queryFn!,
    ),
    // Polled while the window is visible; only the color is exposed so a
    // poll that finds nothing new does not re-render the caller.
    refetchInterval: 5000,
  })

  return { systemAccentColor: query.data }
}
