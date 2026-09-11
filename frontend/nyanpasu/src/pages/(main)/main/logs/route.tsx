import ArticleRounded from '~icons/material-symbols/article-rounded'
import BugReportRounded from '~icons/material-symbols/bug-report-rounded'
import ErrorRounded from '~icons/material-symbols/error-rounded'
import HelpRounded from '~icons/material-symbols/help-rounded'
import InfoRounded from '~icons/material-symbols/info-rounded'
import SearchRounded from '~icons/material-symbols/search-rounded'
import StopCircleRounded from '~icons/material-symbols/stop-circle-rounded'
import WarningRounded from '~icons/material-symbols/warning-rounded'
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
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { createFileRoute, Link, Outlet } from '@tanstack/react-router'
import { LogLevel } from './_modules/consts'

export const Route = createFileRoute('/(main)/main/logs')({
  component: RouteComponent,
  validateSearch: z.object({
    source: z.enum(['core', 'app', 'service']).optional(),
    level: z.enum(LogLevel).nullable().optional(),
  }),
})

const LogLevelIcon = {
  [LogLevel.Trace]: SearchRounded,
  [LogLevel.Debug]: BugReportRounded,
  [LogLevel.Info]: InfoRounded,
  [LogLevel.Warning]: WarningRounded,
  [LogLevel.Error]: ErrorRounded,
  [LogLevel.Fatal]: StopCircleRounded,
  [LogLevel.Unknown]: HelpRounded,
}

const SidebarContent = ({ className, ...props }: ComponentProps<'div'>) => {
  return <div className={cn('p-2', className)} {...props} />
}

const LogLevelButton = ({
  level: inputLevel,
  children,
}: PropsWithChildren<{ level?: LogLevel }>) => {
  const { level, source } = Route.useSearch()

  const Icon = inputLevel ? LogLevelIcon[inputLevel] : ArticleRounded

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
          variant="basic"
          data-active={String((inputLevel ?? null) === (level ?? null))}
          className={cn(
            'h-12 min-w-0 px-3',
            'flex items-center gap-2',
            'text-on-surface-variant data-[active=true]:bg-secondary-container data-[active=true]:text-on-secondary-container',
            'dark:text-on-surface-variant dark:data-[active=true]:bg-secondary-container dark:data-[active=true]:text-on-secondary-container',
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
              source,
              level: inputLevel,
            }}
          >
            <div className="text-md grid size-6 shrink-0 place-content-center">
              <Icon aria-hidden className="size-5" />
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
  return <Outlet />
}

export function LogLevelsLayout({ children }: PropsWithChildren) {
  return (
    <SidebarProvider defaultOpen={false}>
      <div
        className={cn(
          'relative flex min-h-0 w-full flex-1 overflow-hidden',
          'divide-outline-variant divide-x',
        )}
      >
        <Sidebar className="divide-outline-variant z-10 flex flex-col divide-y">
          <SidebarContent className="flex flex-1 flex-col gap-2">
            <LogLevelButton>{m.logs_all_levels()}</LogLevelButton>

            {Object.values(LogLevel).map((item) => (
              <LogLevelButton key={item} level={item}>
                {item}
              </LogLevelButton>
            ))}
          </SidebarContent>

          <SidebarContent className="flex h-16 justify-end">
            <SidebarToggleButton aria-label={m.logs_level_sidebar()} />
          </SidebarContent>
        </Sidebar>

        {children}
      </div>
    </SidebarProvider>
  )
}
