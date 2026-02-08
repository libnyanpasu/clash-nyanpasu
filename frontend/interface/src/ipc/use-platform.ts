import { useEffect, useState } from 'react'
import { unwrapResult } from '@/utils'
import { useQuery } from '@tanstack/react-query'
import { commands } from './bindings'
import { IS_APPIMAGE_QUERY_KEY, OS } from './consts'

export const useIsAppImage = () => {
  const query = useQuery({
    queryKey: [IS_APPIMAGE_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.isAppimage()),
  })

  return {
    ...query,
  }
}

export function useUpdaterSupported() {
  const [supported, setSupported] = useState(false)

  const isAppImage = useIsAppImage()

  useEffect(() => {
    switch (OS) {
      case 'macos':
      case 'windows':
        setSupported(true)
        break
      case 'linux':
        setSupported(!!isAppImage.data)
        break
    }
  }, [isAppImage.data])

  return supported
}
