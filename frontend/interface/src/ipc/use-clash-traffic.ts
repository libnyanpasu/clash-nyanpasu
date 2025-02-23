import { useQuery } from '@tanstack/react-query'
import { CLASH_TRAAFFIC_QUERY_KEY } from './consts'

export type ClashTraffic = {
  up: number
  down: number
}

export const useClashTraffic = () => {
  const query = useQuery<ClashTraffic[]>({
    queryKey: [CLASH_TRAAFFIC_QUERY_KEY],
    queryFn: () => [],
  })

  return query
}
