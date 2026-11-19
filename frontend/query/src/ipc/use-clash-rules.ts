import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

export const useClashRules = () => {
  const api = useQueryApi()
  const query = useQuery(api.queries.clashApiGetRules())

  return {
    ...query,
  }
}
