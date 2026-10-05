import { LazyImage, type LazyImageProps } from '@nyanpasu/ui/lazy-image'
import { useCachedIcon, useTrayIcon } from '@nyanpasu/query'

type SharedImageProps = Omit<LazyImageProps, 'src'>

export function CacheImage({
  icon,
  ...props
}: SharedImageProps & {
  icon: string
}) {
  const src = icon.trim().startsWith('<svg')
    ? `data:image/svg+xml;base64,${btoa(icon)}`
    : icon
  const query = useCachedIcon(src.startsWith('http') ? src : null)

  return (
    <LazyImage
      src={src.startsWith('http') ? query.data?.data_url : src}
      {...props}
    />
  )
}

export function TrayImage({
  mode,
  version,
  ...props
}: SharedImageProps & {
  mode: 'system_proxy' | 'tun' | 'normal'
  version?: number
}) {
  const query = useTrayIcon(mode, version)

  return <LazyImage src={query.data?.data_url} {...props} />
}
