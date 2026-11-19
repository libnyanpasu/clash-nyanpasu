import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

export const useServicePrompt = () => {
  const api = useQueryApi()
  const query = useQuery(api.queries.getServiceInstallPrompt())

  return {
    ...query,
  }
}
