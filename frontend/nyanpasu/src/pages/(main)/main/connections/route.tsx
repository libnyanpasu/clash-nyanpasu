import ListRounded from '~icons/material-symbols/lists-rounded'
import { ComponentProps, PropsWithChildren, ReactNode, useMemo } from 'react'
import z from 'zod'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Sidebar,
  SidebarLabelItem,
  SidebarProvider,
  SidebarToggleButton,
  useSidebar,
} from '@/components/ui/slider-sidebar'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useIsMobileOrTablet } from '@/hooks/use-is-moblie'
import { m } from '@/paraglide/messages'
import { useClashRules } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { createFileRoute, Link, Outlet } from '@tanstack/react-router'
import ProxyIcon from '../rules/_modules/proxy-icon'

export const Route = createFileRoute('/(main)/main/connections')({
  component: RouteComponent,
  validateSearch: z.object({
    proxy: z.string().optional().nullable(),
  }),
})

const SidebarContent = ({ className, ...props }: ComponentProps<'div'>) => {
  return <div className={cn('p-2', className)} {...props} />
}

const Item = ({
  item,
  children,
  icon,
}: PropsWithChildren<{
  item?: string
  icon?: ReactNode
}>) => {
  const { proxy } = Route.useSearch()

  const { open, setOpen } = useSidebar()

  const isMobileOrTablet = useIsMobileOrTablet()

  const handleClick = () => {
    if (isMobileOrTablet) {
      setOpen(false)
    }
  }

  return (
    <Tooltip open={open ? false : undefined}>
      <TooltipTrigger asChild>
        <Button
          variant="fab"
          data-active={String(item === proxy)}
          className={cn(
            'h-12 min-w-0 px-3',
            'flex items-center gap-2',
            'data-[active=true]:bg-surface-variant/50',
            'data-[active=false]:bg-transparent',
            'data-[active=false]:shadow-none',
            'data-[active=false]:hover:shadow-none',
            'data-[active=false]:hover:bg-surface-variant/30',
          )}
          onClick={handleClick}
          asChild
        >
          <Link
            to="."
            search={{
              proxy: item,
            }}
          >
            <div className="text-md grid size-6 shrink-0 place-content-center">
              {icon}
            </div>

            <SidebarLabelItem>{children}</SidebarLabelItem>
          </Link>
        </Button>
      </TooltipTrigger>

      <TooltipContent side="right">
        <p>{children}</p>
      </TooltipContent>
    </Tooltip>
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
    <SidebarContent className="flex flex-col gap-2">
      <Item icon={<ListRounded />}>{m.connections_all_connections()}</Item>

      {allProxy.map((item) => (
        <Item key={item} item={item} icon={<ProxyIcon groupName={item} />}>
          {item}
        </Item>
      ))}
    </SidebarContent>
  )
}

function RouteComponent() {
  return (
    <SidebarProvider defaultOpen={false}>
      <div
        className={cn(
          'divide-outline-variant relative flex h-full min-h-0 w-full divide-x overflow-hidden',
        )}
      >
        <Sidebar className="divide-outline-variant z-10 flex flex-col divide-y">
          <ScrollArea className="min-h-0 w-full flex-1 [&>div>div]:block!">
            <ProxySelector />
          </ScrollArea>

          <SidebarContent className="flex h-16 justify-end">
            <SidebarToggleButton />
          </SidebarContent>
        </Sidebar>

        <Outlet />
      </div>
    </SidebarProvider>
  )
}
