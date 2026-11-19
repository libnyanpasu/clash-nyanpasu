import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

export const useIsAppImage = () => {
  const api = useQueryApi()
  return useQuery({
    ...api.queries.isAppimage(),
    staleTime: Infinity,
  })
}

export const useIsPortable = () => {
  const api = useQueryApi()
  return useQuery({
    ...api.queries.isPortable(),
    staleTime: Infinity,
  })
}
