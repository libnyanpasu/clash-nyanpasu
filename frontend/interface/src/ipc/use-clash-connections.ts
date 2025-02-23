import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useClashAPI } from '../service/clash-api'

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
  const queryClient = useQueryClient()

  const clashApi = useClashAPI()

  const query = useQuery<ClashConnection[]>({
    queryKey: ['clash-connections'],
    queryFn: () => [],
  })

  const deleteConnections = useMutation({
    mutationFn: async (id?: string) => {
      await clashApi.deleteConnections(id)

      const currentData = queryClient.getQueryData([
        'clash-connections',
      ]) as ClashConnection[]

      if (id) {
        const lastConnections = currentData.at(-1)?.connections

        if (lastConnections) {
          const filteredConnections = lastConnections.filter(
            (conn) => conn.id !== id,
          )

          const lastData = {
            ...currentData.at(-1)!,
            connections: filteredConnections,
          }

          queryClient.setQueryData(
            ['clash-connections'],
            [...currentData.slice(0, -1), lastData],
          )
        }
      } else {
        const lastData = currentData.at(-1)

        if (lastData) {
          const { downloadTotal, uploadTotal } = lastData

          queryClient.setQueryData(
            ['clash-connections'],
            [
              ...currentData.slice(0, -1),
              {
                downloadTotal,
                uploadTotal,
              },
            ],
          )
        }
      }
    },
  })

  return {
    query,
    deleteConnections,
  }
}
