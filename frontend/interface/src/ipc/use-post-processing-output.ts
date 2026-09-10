import { useQuery } from '@tanstack/react-query'
import { queries } from './bindings'
import { unwrapQueryOptions } from './query-options'

/**
 * Custom hook for fetching post-processing output using React Query.
 * Another name is chains/script logs.
 *
 * This hook queries post-processing output data using a predefined query key
 * and fetches the data through the `commands.getPostprocessingOutput` command.
 * The result is unwrapped using the `unwrapResult` utility function.
 */
export const usePostProcessingOutput = () => {
  const query = useQuery(
    unwrapQueryOptions(
      queries.getPostprocessingOutput(),
      queries.getPostprocessingOutput().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
