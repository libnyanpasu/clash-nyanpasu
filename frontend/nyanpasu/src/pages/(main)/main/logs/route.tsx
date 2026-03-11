import MenuOpenRounded from '~icons/material-symbols/menu-open-rounded'
import { ComponentProps, PropsWithChildren } from 'react'
import { z } from 'zod'
import { Button } from '@/components/ui/button'
import {
  Sidebar,
  SidebarLabelItem,
  SidebarProvider,
  useSidebar,
} from '@/components/ui/slider-sidebar'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { cn } from '@nyanpasu/ui'
import { createFileRoute, Link, Outlet } from '@tanstack/react-router'
import { LogLevel } from './_modules/consts'

export const Route = createFileRoute('/(main)/main/logs')({
  component: RouteComponent,
  validateSearch: z.object({
    level: z.enum(LogLevel).nullable().optional(),
  }),
})

const LogLevelIcon = {
  [LogLevel.Debug]: () => '🐛',
  [LogLevel.Info]: () => 'ℹ️',
  [LogLevel.Warning]: () => '⚠️',
  [LogLevel.Error]: () => '❌',
} satisfies Record<LogLevel, React.FC>

const SidebarContent = ({ className, ...props }: ComponentProps<'div'>) => {
  return <div className={cn('p-2', className)} {...props} />
}

const SidebarToggleButton = () => {
  const { open, setOpen } = useSidebar()

  return (
    <Button
      className="flex size-12 min-w-0 items-center gap-2 rounded-2xl px-3 text-left"
      variant="raised"
      onClick={() => setOpen(!open)}
    >
      <MenuOpenRounded className="size-6 shrink-0" />
    </Button>
  )
}

const LogLevelButton = ({
  level: inputLevel,
  children,
}: PropsWithChildren<{ level?: LogLevel }>) => {
  const { level } = Route.useSearch()

  const Icon = inputLevel ? LogLevelIcon[inputLevel] : () => '📋'

  const { open } = useSidebar()

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="fab"
          data-active={String(inputLevel === level)}
          className={cn(
            'h-12 min-w-0 px-3',
            'flex items-center gap-2',
            'data-[active=true]:bg-surface-variant/50',
            'data-[active=false]:bg-transparent',
            'data-[active=false]:shadow-none',
            'data-[active=false]:hover:shadow-none',
            'data-[active=false]:hover:bg-surface-variant/30',
          )}
          asChild
        >
          <Link
            to="."
            search={{
              level: inputLevel,
            }}
          >
            <div className="text-md grid size-6 shrink-0 place-content-center">
              <Icon />
            </div>

            <SidebarLabelItem className="capitalize">
              {children}
            </SidebarLabelItem>
          </Link>
        </Button>
      </TooltipTrigger>

      {!open && (
        <TooltipContent side="right">
          <p className="capitalize">{children}</p>
        </TooltipContent>
      )}
    </Tooltip>
  )
}

function RouteComponent() {
  return (
    <SidebarProvider defaultOpen={false}>
      <div
        className={cn(
          'divide-outline-variant flex h-full min-h-0 w-full divide-x overflow-hidden',
        )}
      >
        <Sidebar className="divide-outline-variant flex flex-col divide-y">
          <SidebarContent className="flex flex-1 flex-col gap-2">
            <LogLevelButton>All</LogLevelButton>

            {Object.values(LogLevel).map((item) => (
              <LogLevelButton key={item} level={item}>
                {item}
              </LogLevelButton>
            ))}
          </SidebarContent>

          <SidebarContent className="flex h-16 justify-end">
            <SidebarToggleButton />
          </SidebarContent>
        </Sidebar>

        <Outlet />
      </div>
    </SidebarProvider>
  )
}
