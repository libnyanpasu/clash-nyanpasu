import { ComponentProps, useMemo } from 'react'
import { useServerPort } from '@nyanpasu/interface'
import { LazyImage } from '@nyanpasu/ui'

export default function Image({
  icon,
  ...porps
}: Omit<ComponentProps<typeof LazyImage>, 'src'> & {
  icon: string
}) {
  const serverPort = useServerPort()

  const src = icon.trim().startsWith('<svg')
    ? `data:image/svg+xml;base64,${btoa(icon)}`
    : icon

  const cachedUrl = useMemo(() => {
    if (!src.startsWith('http')) {
      return src
    }

    return `http://localhost:${serverPort}/cache/icon?url=${btoa(src)}`
  }, [src, serverPort])

  return <LazyImage src={cachedUrl} {...porps} />
}
