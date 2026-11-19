import { useMutation, useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

/** Every HTTP lookup provider the user may accept, with its endpoints. */
export const useIpLookupProviders = () => {
  const api = useQueryApi()

  return useQuery(api.queries.getIpLookupProviders())
}

/** Asks a STUN server over each transport and family. Desktop only. */
export const useStunServerTest = () => {
  const api = useQueryApi()

  return useMutation({
    mutationKey: api.mutations.testStunServer().mutationKey,
    mutationFn: async ({ host, port }: { host: string; port: number }) =>
      await api.testStunServer(host, port),
    retry: false,
  })
}
