import { ComponentProps, useMemo } from 'react'
import { useServerPort } from '@nyanpasu/interface'
import { LazyImage } from '@nyanpasu/ui'

export function CacheImage({
  icon,
  ...props
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

  return <LazyImage src={cachedUrl} {...props} />
}

export function TrayImage({
  mode,
  version,
  ...props
}: Omit<ComponentProps<typeof LazyImage>, 'src'> & {
  mode: 'system_proxy' | 'tun' | 'normal'
  version?: number
}) {
  const serverPort = useServerPort()

  const src = `http://localhost:${serverPort}/tray/icon?mode=${mode}${version !== undefined ? `&v=${version}` : ''}`

  return <LazyImage src={src} {...props} />
}
