import { ComponentProps, PropsWithChildren } from 'react'
import { z } from 'zod'
import { Button } from '@/components/ui/button'
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
import { cn } from '@nyanpasu/utils'
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

const LogLevelButton = ({
  level: inputLevel,
  children,
}: PropsWithChildren<{ level?: LogLevel }>) => {
  const { level } = Route.useSearch()

  const Icon = inputLevel ? LogLevelIcon[inputLevel] : () => '📋'

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
          onClick={handleClick}
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

      <TooltipContent side="right">
        <p className="capitalize">{children}</p>
      </TooltipContent>
    </Tooltip>
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
