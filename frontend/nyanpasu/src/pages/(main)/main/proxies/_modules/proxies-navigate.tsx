import GroupSummary from '@/components/proxies/group-summary'
import { Button } from '@/components/ui/button'
import { CacheImage } from '@/components/ui/image'
import { useClashProxies } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { Link, useLocation } from '@tanstack/react-router'

export default function ProxiesNavigate() {
  const {
    proxies: { data: proxies },
  } = useClashProxies()

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
            <div className="flex w-full min-w-0 items-center gap-2.5">
              {group.icon && (
                <div className="size-8 shrink-0">
                  <CacheImage
                    icon={group.icon}
                    className="size-8"
                    loadingClassName="rounded-full"
                  />
                </div>
              )}

              <div className="flex min-w-0 flex-1 flex-col gap-1">
                <div
                  className="truncate text-sm font-medium"
                  title={group.name}
                >
                  {group.name}
                </div>
                <GroupSummary group={group} proxies={proxies} />
              </div>
            </div>
          </Link>
        </Button>
      ))}
    </div>
  )
}
