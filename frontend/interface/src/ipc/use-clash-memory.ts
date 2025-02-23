import { useQuery } from '@tanstack/react-query'

export type ClashMemory = {
  inuse: number
  oslimit: number
}

export const useClashMemory = () => {
  const query = useQuery<ClashMemory[]>({
    queryKey: ['clash-memory'],
    queryFn: () => [],
  })

  return query
}
