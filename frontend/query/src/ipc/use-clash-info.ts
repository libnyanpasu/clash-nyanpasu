import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

/**
 * A hook that retrieves and returns clash information using react-query.
 *
 * This hook leverages the useQuery hook to asynchronously fetch clash information by invoking
 * the getClashInfo command. The fetched result is processed via unwrapResult before being returned
 * alongside the query's state and metadata.
 *
 * @returns An object containing the properties of the query returned by useQuery, including loading,
 * error states, and the fetched data.
 */
export const useClashInfo = () => {
  const api = useQueryApi()
  const query = useQuery(
    unwrapQueryOptions(
      api.queries.getClashInfo(),
      api.queries.getClashInfo().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
