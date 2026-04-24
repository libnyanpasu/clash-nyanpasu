import { useMount } from 'ahooks'
import dayjs from 'dayjs'
import { cn } from '@nyanpasu/utils'
import {
  createRootRoute,
  ErrorComponentProps,
  Outlet,
} from '@tanstack/react-router'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import 'dayjs/locale/ru'
import 'dayjs/locale/zh-cn'
import 'dayjs/locale/zh-tw'
import customParseFormat from 'dayjs/plugin/customParseFormat'
import relativeTime from 'dayjs/plugin/relativeTime'
import { lazy } from 'react'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import { LanguageProvider } from '@/components/providers/language-provider'
import { ExperimentalThemeProvider } from '@/components/providers/theme-provider'
import { events, NyanpasuProvider } from '@nyanpasu/interface'

dayjs.extend(relativeTime)
dayjs.extend(customParseFormat)

const appWindow = getCurrentWebviewWindow()

export const Catch = ({ error }: ErrorComponentProps) => {
  return (
    <div className={cn('h-dvh bg-black text-white', 'flex flex-col gap-4 p-4')}>
      <div
        className="fixed top-0 left-0 z-10 h-6 w-full"
        data-tauri-drag-region
      />

      <h1 data-tauri-drag-region>Oops!</h1>

      <p>Something went wrong... Caught in error boundary.</p>

      <pre className="overflow-x-auto font-mono whitespace-pre-wrap select-text">
        {error.message}
        {error.stack}
      </pre>

      <div className="flex items-center gap-2">
        <button
          className="cursor-pointer bg-zinc-900 px-3 py-2 text-zinc-100"
          onClick={() => window.location.reload()}
        >
          Reload Resource
        </button>

        <button
          className="cursor-pointer bg-zinc-900 px-3 py-2 text-zinc-100"
          onClick={() => appWindow.close()}
        >
          Close Window
        </button>
      </div>
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
  useMount(() => {
    Promise.all([
      appWindow.show(),
      appWindow.unminimize(),
      appWindow.setFocus(),
    ]).finally(() => {
      events.reactAppMountedEvent.emit(null)
    })
  })

  return (
    <NyanpasuProvider>
      <BlockTaskProvider>
        <LanguageProvider>
          <ExperimentalThemeProvider>
            <Outlet />
          </ExperimentalThemeProvider>

          <TanStackRouterDevtools />
        </LanguageProvider>
      </BlockTaskProvider>
    </NyanpasuProvider>
  )
}
