import ListRounded from '~icons/material-symbols/lists-rounded'
import { useMemo, type PropsWithChildren, type ReactNode } from 'react'
import z from 'zod'
import {
  NavigationSidebar,
  NavigationSidebarContent,
  NavigationSidebarItem,
  NavigationSidebarToggle,
} from '@nyanpasu/ui/navigation-sidebar'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { SidebarProvider } from '@nyanpasu/ui/slider-sidebar'
import { keepReturn } from '@/components/router/cross-navigation'
import { m } from '@/paraglide/messages'
import { useClashRules } from '@nyanpasu/query'
import { createFileRoute, Link, Outlet } from '@tanstack/react-router'
import { dataPageLayoutVariants } from '../_modules/data-page'
import { RANGES, searchFilterSchema } from '../_modules/traffic-filters'
import ProxyIcon from '../rules/_modules/proxy-icon'

export const Route = createFileRoute('/(main)/main/connections')({
  component: RouteComponent,
  validateSearch: z.object({
    proxy: z.string().optional().nullable(),
    scope: z.enum(['all', 'active', 'closed']).optional(),
    range: z.enum(RANGES).optional(),
    filters: z.array(searchFilterSchema).optional(),
    q: z.string().optional(),
  }),
})

const Item = ({
  item,
  children,
  icon,
}: PropsWithChildren<{
  item?: string
  icon: ReactNode
}>) => {
  const { proxy } = Route.useSearch()

  return (
    <NavigationSidebarItem
      active={(item ?? null) === (proxy ?? null)}
      icon={icon}
      renderLink={(content) => (
        <Link
          from={Route.fullPath}
          to="."
          search={(previous) => ({
            ...previous,
            proxy: item,
          })}
          state={keepReturn}
        >
          {content}
        </Link>
      )}
    >
      {children}
    </NavigationSidebarItem>
  )
}

const ProxySelector = () => {
  const { data } = useClashRules()

  const allProxy = useMemo(() => {
    const proxies =
      data?.rules
        .map((rule) => rule.proxy)
        .filter((proxy): proxy is string => !!proxy) ?? []

    return [...new Set(proxies)]
  }, [data])

  return (
    <NavigationSidebarContent className="pb-16">
      <Item icon={<ListRounded />}>{m.connections_all_connections()}</Item>

      {allProxy.map((item) => (
        <Item key={item} item={item} icon={<ProxyIcon groupName={item} />}>
          {item}
        </Item>
      ))}
    </NavigationSidebarContent>
  )
}

function RouteComponent() {
  return (
    <SidebarProvider defaultOpen={false}>
      <div className={dataPageLayoutVariants()}>
        <NavigationSidebar>
          <ScrollArea className="min-h-0 w-full flex-1 [&>div>div]:block!">
            <ProxySelector />
          </ScrollArea>

          <NavigationSidebarToggle aria-label={m.connections_all_connections()}>
            {m.navigation_sidebar_collapse()}
          </NavigationSidebarToggle>
        </NavigationSidebar>

        <Outlet />
      </div>
    </SidebarProvider>
  )
}
