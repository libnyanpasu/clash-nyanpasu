import { unwrapResult } from '@/utils'
import { useQuery } from '@tanstack/react-query'
import { commands } from './bindings'

/**
 * Custom hook to fetch and manage the system service status using TanStack Query.
 *
 * @returns An object containing the query result for the system service status.
 */
export const useSystemService = () => {
  const query = useQuery({
    queryKey: ['system-service'],
    queryFn: async () => {
      return unwrapResult(await commands.statusService())
    },
  })

  return {
    query,
  }
}
