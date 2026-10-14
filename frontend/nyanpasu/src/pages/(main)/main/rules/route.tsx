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
import ProxyIcon from './_modules/proxy-icon'
import { RULE_SORTS } from './_modules/rule-row'

export const Route = createFileRoute('/(main)/main/rules')({
  component: RouteComponent,
  validateSearch: z.object({
    proxy: z.string().optional().nullable(),
    q: z.string().optional(),
    // The rule order is the default, so it is never written.
    sort: z.enum(RULE_SORTS).exclude(['index']).optional(),
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
      <Item icon={<ListRounded />}>{m.rules_list_all_proxies()}</Item>

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

          <NavigationSidebarToggle aria-label={m.rules_list_all_proxies()}>
            {m.navigation_sidebar_collapse()}
          </NavigationSidebarToggle>
        </NavigationSidebar>

        <Outlet />
      </div>
    </SidebarProvider>
  )
}
