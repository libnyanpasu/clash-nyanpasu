import { useQuery } from '@tanstack/react-query'

export type ClashTraffic = {
  up: number
  down: number
}

export const useClashTraffic = () => {
  const query = useQuery<ClashTraffic[]>({
    queryKey: ['clash-traffic'],
    queryFn: () => [],
  })

  return query
}
