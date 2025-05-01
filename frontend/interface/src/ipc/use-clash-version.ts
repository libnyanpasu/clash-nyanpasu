import { useQuery } from '@tanstack/react-query'
import { useClashAPI } from '../service/clash-api'
import { CLASH_VERSION_QUERY_KEY } from './consts'

export const useClashVersion = () => {
  const { version } = useClashAPI()

  const query = useQuery({
    queryKey: [CLASH_VERSION_QUERY_KEY],
    queryFn: async () => {
      return await version()
    },
  })

  return query
}
