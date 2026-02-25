import { getServerPort } from '@/service'
import { useQuery } from '@tanstack/react-query'
import { SERVER_PORT_QUERY_KEY } from './consts'

export const useServerPort = () => {
  const { data: serverPort } = useQuery({
    queryKey: [SERVER_PORT_QUERY_KEY],
    queryFn: () => getServerPort(),
  })

  return serverPort
}
