import { useQuery, useQueryClient } from '@tanstack/react-query'
import { CLASH_MEMORY_QUERY_KEY } from './consts'

export type ClashMemory = {
  inuse: number
  oslimit: number
}

export const useClashMemory = () => {
  const queryClient = useQueryClient()

  const query = useQuery<ClashMemory[]>({
    queryKey: [CLASH_MEMORY_QUERY_KEY],
    queryFn: () => {
      return (
        queryClient.getQueryData<ClashMemory[]>([CLASH_MEMORY_QUERY_KEY]) || []
      )
    },
  })

  return query
}
