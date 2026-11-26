import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

/**
 * A hook that retrieves and returns clash information using react-query.
 *
 * This hook leverages the useQuery hook to asynchronously fetch clash information by invoking
 * the getClashInfo command. The fetched result is returned alongside the query's state and
 * metadata.
 *
 * @returns An object containing the properties of the query returned by useQuery, including loading,
 * error states, and the fetched data.
 */
export const useClashInfo = (options?: { enabled?: boolean }) => {
  const api = useQueryApi()
  const infoOptions = api.queries.getClashInfo()
  const query = useQuery({
    ...infoOptions,
    enabled: options?.enabled,
  })

  return {
    ...query,
  }
}
