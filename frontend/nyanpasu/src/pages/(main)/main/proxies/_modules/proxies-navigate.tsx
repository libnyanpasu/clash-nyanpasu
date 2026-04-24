import { Button } from '@/components/ui/button'
import { CacheImage } from '@/components/ui/image'
import { useClashProxies } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { Link, useLocation } from '@tanstack/react-router'

export default function ProxiesNavigate() {
  const {
    proxies: { data: proxies },
  } = useClashProxies()

  console.log(proxies)

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
            to="/main/proxies/group/$name"
            params={{
              name: group.name,
            }}
          >
            <div className="flex items-center gap-2.5">
              {group.icon && (
                <div className="size-8">
                  <CacheImage
                    icon={group.icon}
                    className="size-8"
                    loadingClassName="rounded-full"
                  />
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
