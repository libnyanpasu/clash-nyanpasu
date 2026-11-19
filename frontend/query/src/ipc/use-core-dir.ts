import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

export const useCoreDir = () => {
  const api = useQueryApi()
  const query = useQuery(api.queries.getCoreDir())

  return {
    ...query,
  }
}
