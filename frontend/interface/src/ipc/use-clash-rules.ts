import { useQuery } from '@tanstack/react-query'
import { useClashAPI } from '../service/clash-api'
import { CLASH_RULES_QUERY_KEY } from './consts'

export const useClashRules = () => {
  const { rules } = useClashAPI()

  const query = useQuery({
    queryKey: [CLASH_RULES_QUERY_KEY],
    queryFn: async () => {
      return await rules()
    },
  })

  return {
    ...query,
  }
}
