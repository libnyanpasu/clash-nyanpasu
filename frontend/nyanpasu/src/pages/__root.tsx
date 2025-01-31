import { useMount } from 'ahooks'
import dayjs from 'dayjs'
import AppContainer from '@/components/app/app-container'
import LocalesProvider from '@/components/app/locales-provider'
import MutationProvider from '@/components/layout/mutation-provider'
import NoticeProvider from '@/components/layout/notice-provider'
import PageTransition from '@/components/layout/page-transition'
import SchemeProvider from '@/components/layout/scheme-provider'
import {
  ThemeModeProvider,
  useCustomTheme,
} from '@/components/layout/use-custom-theme'
import LogProvider from '@/components/logs/log-provider'
import UpdaterDialog from '@/components/updater/updater-dialog-wrapper'
import { useNyanpasuStorageSubscribers } from '@/hooks/use-store'
import useUpdater from '@/hooks/use-updater'
import { FileRouteTypes } from '@/routeTree.gen'
import { atomIsDrawer, memorizedRoutePathAtom } from '@/store'
import { CssBaseline, useTheme } from '@mui/material'
import { StyledEngineProvider, ThemeProvider } from '@mui/material/styles'
import { cn, useBreakpoint } from '@nyanpasu/ui'
import {
  createRootRoute,
  ErrorComponentProps,
  useLocation,
} from '@tanstack/react-router'
import { emit } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import 'dayjs/locale/ru'
import 'dayjs/locale/zh-cn'
import 'dayjs/locale/zh-tw'
import customParseFormat from 'dayjs/plugin/customParseFormat'
import relativeTime from 'dayjs/plugin/relativeTime'
import { useAtom, useSetAtom } from 'jotai'
import { lazy, useEffect } from 'react'
import { SWRConfig } from 'swr'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import styles from './-__root.module.scss'

dayjs.extend(relativeTime)
dayjs.extend(customParseFormat)

export const Catch = ({ error }: ErrorComponentProps) => {
  const theme = useTheme()

  return (
    <div
      className={cn(styles.oops, theme.palette.mode === 'dark' && styles.dark)}
    >
      <h1>Oops!</h1>
      <p>Something went wrong... Caught at _root error boundary.</p>
      <pre>
        {error.message}
        {error.stack}
      </pre>
    </div>
  )
}

export const Pending = () => <div>Loading from _root...</div>

const TanStackRouterDevtools = import.meta.env.PROD
  ? () => null // Render nothing in production
  : lazy(() =>
      // Lazy load in development
      import('@tanstack/router-devtools').then((res) => ({
        default: res.TanStackRouterDevtools,
        // For Embedded Mode
        // default: res.TanStackRouterDevtoolsPanel
      })),
    )

export const Route = createRootRoute({
  component: App,
  errorComponent: Catch,
  pendingComponent: Pending,
})

const queryClient = new QueryClient()

export default function App() {
  const { theme } = useCustomTheme()

  const breakpoint = useBreakpoint()

  const setMemorizedPath = useSetAtom(memorizedRoutePathAtom)
  const pathname = useLocation({
    select: (location) => location.pathname,
  })

  useEffect(() => {
    if (pathname !== '/') {
      setMemorizedPath(pathname as FileRouteTypes['fullPaths'])
    }
  }, [pathname, setMemorizedPath])

  const [isDrawer, setIsDrawer] = useAtom(atomIsDrawer)

  useUpdater()
  useNyanpasuStorageSubscribers()

  useEffect(() => {
    setIsDrawer(breakpoint === 'sm' || breakpoint === 'xs')
  }, [breakpoint, setIsDrawer])

  useMount(() => {
    const appWindow = getCurrentWebviewWindow()
    Promise.all([
      appWindow.show(),
      appWindow.unminimize(),
      appWindow.setFocus(),
    ]).finally(() => emit('react_app_mounted'))
  })

  return (
    <QueryClientProvider client={queryClient}>
      <SWRConfig
        value={{
          errorRetryCount: 5,
          revalidateOnMount: true,
          revalidateOnFocus: true,
          refreshInterval: 5000,
        }}
      >
        <StyledEngineProvider injectFirst>
          <ThemeProvider theme={theme}>
            <CssBaseline />
            <ThemeModeProvider />
            <LogProvider />
            <LocalesProvider />
            <MutationProvider />
            <NoticeProvider />
            <SchemeProvider />
            <UpdaterDialog />

            <AppContainer isDrawer={isDrawer}>
              <PageTransition
                className={cn('absolute inset-4 top-10', !isDrawer && 'left-0')}
              />
              <TanStackRouterDevtools />
            </AppContainer>
          </ThemeProvider>
        </StyledEngineProvider>
      </SWRConfig>
    </QueryClientProvider>
  )
}
