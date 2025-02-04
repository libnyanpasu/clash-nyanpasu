import { unwrapResult } from '@/utils'
import { useQuery } from '@tanstack/react-query'
import { commands } from './bindings'

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
  const query = useQuery({
    queryKey: ['clash-info'],
    queryFn: async () => {
      return unwrapResult(await commands.getClashInfo())
    },
  })

  return {
    ...query,
  }
}
