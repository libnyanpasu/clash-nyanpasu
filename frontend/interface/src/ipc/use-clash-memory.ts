import { useQuery } from '@tanstack/react-query'
import { CLASH_MEMORY_QUERY_KEY } from './consts'

export type ClashMemory = {
  inuse: number
  oslimit: number
}

export const useClashMemory = () => {
  const query = useQuery<ClashMemory[]>({
    queryKey: [CLASH_MEMORY_QUERY_KEY],
    queryFn: () => [],
  })

  return query
}
