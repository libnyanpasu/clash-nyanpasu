import { unwrapResult } from '@nyanpasu/rpc'
import {
  type ReleaseChannel,
  type ReleaseChannelInfo,
} from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

const queryKey = ['getReleaseChannel'] as const

export const useReleaseChannel = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()

  const query = useQuery({
    queryKey,
    queryFn: async () => unwrapResult(await api.getReleaseChannel()),
  })

  const mutation = useMutation({
    onMutate: () => queryClient.cancelQueries({ queryKey }),
    mutationFn: async (channel: ReleaseChannel) =>
      unwrapResult(await api.setReleaseChannel(channel)),
    onSuccess: (_, channel) => {
      queryClient.setQueryData<ReleaseChannelInfo>(
        queryKey,
        (info) => info && { ...info, current: channel },
      )
    },
  })

  return { query, mutation }
}
