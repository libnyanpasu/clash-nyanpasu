import { unwrapResult } from '@/utils'
import { useQuery } from '@tanstack/react-query'
import { commands } from './bindings'
import { NYANPASU_POST_PROCESSING_QUERY_KEY } from './consts'

/**
 * Custom hook for fetching post-processing output using React Query.
 * Another name is chains/script logs.
 *
 * This hook queries post-processing output data using a predefined query key
 * and fetches the data through the `commands.getPostprocessingOutput` command.
 * The result is unwrapped using the `unwrapResult` utility function.
 */
export const usePostProcessingOutput = () => {
  const query = useQuery({
    queryKey: [NYANPASU_POST_PROCESSING_QUERY_KEY],
    queryFn: async () => {
      return unwrapResult(await commands.getPostprocessingOutput())
    },
  })

  return {
    ...query,
  }
}
