import { PropsWithChildren, useEffect } from 'react'
import useCustomCss from '@/hooks/use-custom-css'

export default function CustomCssProvider({ children }: PropsWithChildren) {
  const { tryInjectCss, isLoading } = useCustomCss()

  useEffect(() => {
    if (isLoading) {
      return
    }

    tryInjectCss()
  }, [isLoading, tryInjectCss])

  return <>{children}</>
}
