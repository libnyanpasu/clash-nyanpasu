import { unwrapResult } from '@nyanpasu/rpc'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

/** Every HTTP lookup provider the user may accept, with its endpoints. */
export const useIpLookupProviders = () => {
  const api = useQueryApi()

  return useQuery(
    unwrapQueryOptions(
      api.queries.getIpLookupProviders(),
      api.queries.getIpLookupProviders().queryFn!,
    ),
  )
}

/** Asks a STUN server over each transport and family. Desktop only. */
export const useStunServerTest = () => {
  const api = useQueryApi()

  return useMutation({
    mutationKey: api.mutations.testStunServer.mutationKey,
    mutationFn: async ({ host, port }: { host: string; port: number }) =>
      unwrapResult(await api.testStunServer(host, port)),
    retry: false,
  })
}
