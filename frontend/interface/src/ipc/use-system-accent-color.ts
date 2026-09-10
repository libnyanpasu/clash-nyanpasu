import { useQuery } from '@tanstack/react-query'
import { queries } from './bindings'
import { unwrapQueryOptions } from './query-options'

export const useSystemAccentColor = () => {
  const query = useQuery({
    ...unwrapQueryOptions(
      queries.getSystemAccentColor(),
      queries.getSystemAccentColor().queryFn!,
    ),
    refetchInterval: 5000,
    refetchIntervalInBackground: true,
  })

  return {
    systemAccentColor: query.data,
    ...query,
  }
}
