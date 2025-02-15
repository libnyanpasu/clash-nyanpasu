import { useUpdateEffect } from 'ahooks'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useClashWebSocket } from './use-clash-web-socket'

const MAX_CONNECTIONS_HISTORY = 32

export type ClashConnection = {
  downloadTotal: number
  uploadTotal: number
  memory?: number
  connections?: ClashConnectionItem[]
}

export type ClashConnectionItem = {
  id: string
  metadata: ClashConnectionMetadata
  upload: number
  download: number
  start: string
  chains: string[]
  rule: string
  rulePayload: string
}

export type ClashConnectionMetadata = {
  network: string
  type: string
  host: string
  sourceIP: string
  sourcePort: string
  destinationPort: string
  destinationIP?: string
  destinationIPASN?: string
  process?: string
  processPath?: string
  dnsMode?: string
  dscp?: number
  inboundIP?: string
  inboundName?: string
  inboundPort?: string
  inboundUser?: string
  remoteDestination?: string
  sniffHost?: string
  specialProxy?: string
  specialRules?: string
}

export const useClashConnections = () => {
  const { connectionsWS } = useClashWebSocket()

  const queryClient = useQueryClient()

  useUpdateEffect(() => {
    const data = JSON.parse(
      connectionsWS.latestMessage?.data,
    ) as ClashConnection

    const currentData = queryClient.getQueryData([
      'clash-connections',
    ]) as ClashConnection[]

    const newData = [...(currentData || []), data]

    if (newData.length > MAX_CONNECTIONS_HISTORY) {
      newData.shift()
    }

    queryClient.setQueryData(['clash-connections'], newData)
  }, [connectionsWS.latestMessage])

  const query = useQuery<ClashConnection[]>({
    queryKey: ['clash-connections'],
    queryFn: () => [],
  })

  return query
}
