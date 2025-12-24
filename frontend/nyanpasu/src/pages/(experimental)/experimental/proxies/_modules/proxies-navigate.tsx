import { useMemo } from 'react'
import { Button } from '@/components/ui/button'
import { useClashProxies, useServerPort } from '@nyanpasu/interface'
import { cn, LazyImage } from '@nyanpasu/ui'
import { Link, useLocation } from '@tanstack/react-router'

const ProxyGroupIconRender = ({ icon }: { icon: string }) => {
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

  return (
    <LazyImage
      className="size-8"
      loadingClassName="rounded-full"
      src={cachedUrl}
    />
  )
}

export default function ProxiesNavigate() {
  const { data: proxies } = useClashProxies()

  const location = useLocation()

  return (
    <div className="flex flex-col gap-2 p-2">
      {proxies?.groups.map((group) => (
        <Button
          key={group.name}
          variant="fab"
          data-active={String(
            location.pathname.endsWith(`/group/${group.name}`),
          )}
          asChild
        >
          <Link
            className={cn(
              'h-16',
              'flex items-center gap-2',
              'data-[active=true]:bg-surface-variant/80',
              'data-[active=false]:bg-transparent',
              'data-[active=false]:shadow-none',
              'data-[active=false]:hover:shadow-none',
              'data-[active=false]:hover:bg-surface-variant/30',
            )}
            to={`/experimental/proxies/group/${group.name}`}
          >
            <div className="flex items-center gap-2.5">
              {group.icon && (
                <div className="size-8">
                  <ProxyGroupIconRender icon={group.icon} />
                </div>
              )}

              <div className="flex flex-col gap-1">
                <div className="text-sm font-medium">{group.name}</div>
                <div className="text-xs text-zinc-500">
                  {group.now || group.type}
                </div>
              </div>
            </div>
          </Link>
        </Button>
      ))}
    </div>
  )
}
