import { useMount } from 'ahooks'
import dayjs from 'dayjs'
import { ThemeModeProvider } from '@/components/layout/use-custom-theme'
import { useNyanpasuStorageSubscribers } from '@/hooks/use-store'
import { CssBaseline } from '@mui/material'
import { StyledEngineProvider, useColorScheme } from '@mui/material/styles'
import { cn } from '@nyanpasu/ui'
import {
  createRootRoute,
  ErrorComponentProps,
  Outlet,
} from '@tanstack/react-router'
import { emit } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import 'dayjs/locale/ru'
import 'dayjs/locale/zh-cn'
import 'dayjs/locale/zh-tw'
import customParseFormat from 'dayjs/plugin/customParseFormat'
import relativeTime from 'dayjs/plugin/relativeTime'
import { lazy } from 'react'
import { ExperimentalThemeProvider } from '@/components/providers/theme-provider'
import { NyanpasuProvider } from '@nyanpasu/interface'
import styles from './-__root.module.scss'

dayjs.extend(relativeTime)
dayjs.extend(customParseFormat)

export const Catch = ({ error }: ErrorComponentProps) => {
  const { mode } = useColorScheme()
  return (
    <div className={cn(styles.oops, mode === 'dark' && styles.dark)}>
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
      import('@tanstack/react-router-devtools').then((res) => ({
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

export default function App() {
  useNyanpasuStorageSubscribers()

  useMount(() => {
    const appWindow = getCurrentWebviewWindow()
    Promise.all([
      appWindow.show(),
      appWindow.unminimize(),
      appWindow.setFocus(),
    ]).finally(() => emit('react_app_mounted'))
  })

  return (
    <NyanpasuProvider>
      <ExperimentalThemeProvider>
        <StyledEngineProvider injectFirst>
          <ThemeModeProvider>
            <CssBaseline />

            <Outlet />
          </ThemeModeProvider>
        </StyledEngineProvider>
      </ExperimentalThemeProvider>

      <TanStackRouterDevtools />
    </NyanpasuProvider>
  )
}
