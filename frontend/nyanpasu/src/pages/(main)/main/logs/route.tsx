import ArticleRounded from '~icons/material-symbols/article-rounded'
import BugReportRounded from '~icons/material-symbols/bug-report-rounded'
import ErrorRounded from '~icons/material-symbols/error-rounded'
import HelpRounded from '~icons/material-symbols/help-rounded'
import InfoRounded from '~icons/material-symbols/info-rounded'
import SearchRounded from '~icons/material-symbols/search-rounded'
import StopCircleRounded from '~icons/material-symbols/stop-circle-rounded'
import WarningRounded from '~icons/material-symbols/warning-rounded'
import { memo, type PropsWithChildren } from 'react'
import { z } from 'zod'
import {
  NavigationSidebar,
  NavigationSidebarContent,
  NavigationSidebarItem,
  NavigationSidebarToggle,
} from '@nyanpasu/ui/navigation-sidebar'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { SidebarProvider } from '@nyanpasu/ui/slider-sidebar'
import { m } from '@/paraglide/messages'
import { createFileRoute, Link, Outlet } from '@tanstack/react-router'
import { dataPageLayoutVariants } from '../_modules/data-page'
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

const LogLevelButton = ({
  level: inputLevel,
  children,
}: PropsWithChildren<{ level?: LogLevel }>) => {
  const { level, source } = Route.useSearch()

  const Icon = inputLevel ? LogLevelIcon[inputLevel] : ArticleRounded

  return (
    <NavigationSidebarItem
      active={(inputLevel ?? null) === (level ?? null)}
      icon={<Icon className="size-6" />}
      renderLink={(content) => (
        <Link
          to="."
          search={{
            source,
            level: inputLevel,
          }}
        >
          {content}
        </Link>
      )}
    >
      <span className="capitalize">{children}</span>
    </NavigationSidebarItem>
  )
}

function RouteComponent() {
  return <Outlet />
}

// The sidebar holds its own open state, so the page above it can re-render
// for every log without re-rendering the level buttons.
const LogLevelsSidebar = memo(function LogLevelsSidebar() {
  return (
    <SidebarProvider defaultOpen={false}>
      <NavigationSidebar>
        <ScrollArea className="min-h-0 w-full flex-1 [&>div>div]:block!">
          <NavigationSidebarContent className="pb-16">
            <LogLevelButton>{m.logs_all_levels()}</LogLevelButton>

            {Object.values(LogLevel).map((item) => (
              <LogLevelButton key={item} level={item}>
                {item}
              </LogLevelButton>
            ))}
          </NavigationSidebarContent>
        </ScrollArea>

        <NavigationSidebarToggle aria-label={m.logs_level_sidebar()}>
          {m.navigation_sidebar_collapse()}
        </NavigationSidebarToggle>
      </NavigationSidebar>
    </SidebarProvider>
  )
})

export function LogLevelsLayout({ children }: PropsWithChildren) {
  return (
    <div className={dataPageLayoutVariants()}>
      <LogLevelsSidebar />
      {children}
    </div>
  )
}
