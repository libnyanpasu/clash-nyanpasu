import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

/**
 * The desktop-only Linux service snapshot of applied transparent capture and
 * the kernel objects it observes. Polls while the settings page is mounted.
 */
export const useTransparentProxyDiagnostics = (options?: {
  enabled?: boolean
}) => {
  const api = useQueryApi()
  const query = useQuery({
    ...api.queries.getTransparentProxyDiagnostics(),
    enabled: options?.enabled,
    refetchInterval: 5000,
  })

  return {
    data: query.data,
    isLoading: query.isPending,
    isError: query.isError,
    error: query.error,
  }
}
