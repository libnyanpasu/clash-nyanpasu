import { useQuery } from '@tanstack/react-query'
import { unwrapQueryOptions } from './query-options'
import { rpc } from './rpc'

/**
 * Custom hook for fetching post-processing output using React Query.
 * Another name is chains/script logs.
 *
 * This hook rpc.queries post-processing output data using a predefined query key
 * and fetches the data through the `rpc.getPostprocessingOutput` command.
 * The result is unwrapped using the `unwrapResult` utility function.
 */
export const usePostProcessingOutput = () => {
  const query = useQuery(
    unwrapQueryOptions(
      rpc.queries.getPostprocessingOutput(),
      rpc.queries.getPostprocessingOutput().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
