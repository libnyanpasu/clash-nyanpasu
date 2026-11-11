import { LazyImage, type LazyImageProps } from '@nyanpasu/ui/lazy-image'
import { useCachedIcon, useTrayIcon, useWidgetIcon } from '@nyanpasu/query'

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
  const cachedUrl = useCachedIcon(src.startsWith('http') ? src : null)
  const imageUrl = src.startsWith('http') ? cachedUrl : src

  return <LazyImage src={imageUrl} {...props} />
}

export function TrayImage({
  mode,
  version,
  ...props
}: SharedImageProps & {
  mode: 'system_proxy' | 'tun' | 'normal'
  version?: number
}) {
  const src = useTrayIcon(mode, version)

  return <LazyImage src={src} {...props} />
}

export function WidgetImage({
  mode,
  version,
  ...props
}: SharedImageProps & {
  mode: 'system_proxy' | 'tun' | 'normal'
  version?: number
}) {
  const src = useWidgetIcon(mode, version)

  return <LazyImage src={src} {...props} />
}
