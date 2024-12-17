import useSWR, { SWRConfiguration } from 'swr'
import { isAppImage } from '@nyanpasu/interface'

export const useIsAppImage = (config?: Partial<SWRConfiguration>) => {
  return useSWR<boolean>('/api/is_appimage', isAppImage, {
    ...(config || {}),
    revalidateOnFocus: false,
    revalidateOnReconnect: false,
    refreshInterval: 0,
  })
}
