import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

export const useSystemAccentColor = () => {
  const api = useQueryApi()
  const query = useQuery({
    ...api.queries.getSystemAccentColor(),
    // Polled while the window is visible; only the color is exposed so a
    // poll that finds nothing new does not re-render the caller.
    refetchInterval: 5000,
  })

  return { systemAccentColor: query.data }
}
