import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands, type ReleaseChannel } from './bindings'

const queryKey = ['getReleaseChannel'] as const

export const useReleaseChannel = () => {
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey,
    queryFn: async () => unwrapResult(await commands.getReleaseChannel()),
  })
  const mutation = useMutation({
    onMutate: () => queryClient.cancelQueries({ queryKey }),
    mutationFn: async (channel: ReleaseChannel) =>
      unwrapResult(await commands.setReleaseChannel(channel)),
    onSuccess: (_, channel) => {
      queryClient.setQueryData(queryKey, channel)
    },
  })
  return { query, mutation }
}
