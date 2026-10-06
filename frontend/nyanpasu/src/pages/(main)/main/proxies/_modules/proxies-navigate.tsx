import { memo } from 'react'
import { Button } from '@nyanpasu/ui/button'
import GroupSummary from '@/components/proxies/group-summary'
import { CacheImage } from '@/components/ui/image'
import {
  ClashProxiesQuery,
  ClashProxiesQueryGroupItem,
  useClashProxies,
} from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import { Link, useLocation } from '@tanstack/react-router'

// Memoized so that switching groups re-renders only the items whose active
// state changed, not every group in the sidebar.
const ProxiesNavigateItem = memo(function ProxiesNavigateItem({
  group,
  proxies,
  active,
}: {
  group: ClashProxiesQueryGroupItem
  proxies: ClashProxiesQuery
  active: boolean
}) {
  return (
    <Button variant="fab" data-active={String(active)} asChild>
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
        search={(previous) => ({ q: previous.q })}
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
            <div className="truncate text-sm font-medium" title={group.name}>
              {group.name}
            </div>
            <GroupSummary group={group} proxies={proxies} />
          </div>
        </div>
      </Link>
    </Button>
  )
})

export default function ProxiesNavigate() {
  const {
    proxies: { data: proxies },
  } = useClashProxies()

  const location = useLocation()

  return (
    <div className="flex flex-col gap-2 p-2">
      {proxies?.groups.map((group) => (
        <ProxiesNavigateItem
          key={group.name}
          group={group}
          proxies={proxies}
          active={location.pathname.endsWith(`/group/${group.name}`)}
        />
      ))}
    </div>
  )
}
