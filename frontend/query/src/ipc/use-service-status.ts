import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeQuery } from './query-options'

/**
 * The service status for ordinary consumers, read from the safe summary:
 * the daemon's data directories and log locations are not part of it. The
 * service pages that manage the daemon use `useSystemService` instead.
 */
export const useServiceStatus = () => {
  const api = useQueryApi()
  const statusQuery = api.queries.getServiceStatusSummary()

  return useQuery({
    queryKey: statusQuery.queryKey,
    queryFn: () => invokeQuery(statusQuery),
  })
}
