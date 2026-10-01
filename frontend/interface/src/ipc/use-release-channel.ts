import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { rpc } from './rpc'
import { type ReleaseChannel } from './rpc-bindings'

const queryKey = ['getReleaseChannel'] as const

export const useReleaseChannel = () => {
  const queryClient = useQueryClient()

  const query = useQuery({
    queryKey,
    queryFn: async () => unwrapResult(await rpc.getReleaseChannel()),
  })

  const mutation = useMutation({
    onMutate: () => queryClient.cancelQueries({ queryKey }),
    mutationFn: async (channel: ReleaseChannel) =>
      unwrapResult(await rpc.setReleaseChannel(channel)),
    onSuccess: (_, channel) => {
      queryClient.setQueryData(queryKey, channel)
    },
  })

  return { query, mutation }
}
