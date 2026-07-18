import dayjs from 'dayjs'
import { cn } from '@nyanpasu/utils'
import {
  createRootRoute,
  ErrorComponentProps,
  Outlet,
} from '@tanstack/react-router'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import 'dayjs/locale/ko'
import 'dayjs/locale/ru'
import 'dayjs/locale/zh-cn'
import 'dayjs/locale/zh-tw'
import customParseFormat from 'dayjs/plugin/customParseFormat'
import relativeTime from 'dayjs/plugin/relativeTime'
import { lazy, useEffect, useRef } from 'react'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import CustomCssProvider from '@/components/providers/custom-css-provider'
import { LanguageProvider } from '@/components/providers/language-provider'
import { ExperimentalThemeProvider } from '@/components/providers/theme-provider'
import { TooltipProvider } from '@/components/ui/tooltip'
import { useDeepLinkImport } from '@/hooks/use-deep-link-import'
import { m } from '@/paraglide/messages'
import { message } from '@/utils/notification'
import {
  events,
  NyanpasuProvider,
  setMutationDegradationHandler,
  useSettings,
  type Degradation,
  type DegradationPhase,
} from '@nyanpasu/interface'

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

function WindowReveal() {
  const { query } = useSettings()
  const hasRevealed = useRef(false)

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'f') {
        e.preventDefault()
      }
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [])

  useEffect(() => {
    if ((query.isSuccess || query.isError) && !hasRevealed.current) {
      hasRevealed.current = true
      Promise.all([
        appWindow.show(),
        appWindow.unminimize(),
        appWindow.setFocus(),
      ]).finally(() => {
        events.windowReadyEvent.emit({ label: appWindow.label })
      })
    }
  }, [query.isSuccess, query.isError])

  return null
}

function localizeDegradationPhase(phase: DegradationPhase): string {
  switch (phase) {
    case 'legacy_mirror':
      return m.mutation_degradation_phase_legacy_mirror()
    case 'profile_materialization':
      return m.mutation_degradation_phase_profile_materialization()
    case 'runtime_build':
      return m.mutation_degradation_phase_runtime_build()
    case 'runtime_check':
      return m.mutation_degradation_phase_runtime_check()
    case 'runtime_promote':
      return m.mutation_degradation_phase_runtime_promote()
    case 'runtime_publish':
      return m.mutation_degradation_phase_runtime_publish()
    case 'runtime_apply':
      return m.mutation_degradation_phase_runtime_apply()
    case 'core_rollback':
      return m.mutation_degradation_phase_core_rollback()
    case 'system_effect':
      return m.mutation_degradation_phase_system_effect()
    case 'ui_effect':
      return m.mutation_degradation_phase_ui_effect()
    default: {
      const _exhaustive: never = phase
      return String(_exhaustive)
    }
  }
}

function localizeDegradationCode(code: string): string {
  switch (code) {
    case 'journal_invalid':
      return m.mutation_degradation_code_journal_invalid()
    case 'materialization_deferred':
      return m.mutation_degradation_code_materialization_deferred()
    case 'cleanup_deferred':
      return m.mutation_degradation_code_cleanup_deferred()
    case 'runtime_rebuild_failed':
      return m.mutation_degradation_code_runtime_rebuild_failed()
    case 'profile_auto_activation_failed':
      return m.mutation_degradation_code_profile_auto_activation_failed()
    default:
      return m.mutation_degradation_code_unknown({ code })
  }
}

function formatDegradationItem(degradation: Degradation): string {
  return m.mutation_degraded_item({
    phase: localizeDegradationPhase(degradation.phase),
    detail: localizeDegradationCode(degradation.code),
  })
}

function MutationDegradationNotifier() {
  useEffect(
    () =>
      // setMutationDegradationHandler returns a disposer; useEffect cleanup
      // passes it through so StrictMode remount / HMR leave no dangling handler.
      setMutationDegradationHandler((degradations) => {
        if (degradations.length === 0) {
          return
        }

        // Backend `message` is diagnostic-only; primary copy is phase + code.
        for (const degradation of degradations) {
          console.warn('[mutation-degradation]', {
            phase: degradation.phase,
            code: degradation.code,
            retryable: degradation.retryable,
            message: degradation.message,
          })
        }

        const items = degradations.map(formatDegradationItem).join('; ')
        message(m.mutation_degraded_summary({ items }), {
          title: m.mutation_degraded_title(),
          kind: 'warning',
        }).catch((error) => {
          console.error('[mutation-degradation] failed to show warning', error)
        })
      }),
    [],
  )
  return null
}

function DeepLinkImport() {
  useDeepLinkImport()
  return null
}

export default function App() {
  return (
    <NyanpasuProvider>
      <BlockTaskProvider>
        <LanguageProvider>
          <ExperimentalThemeProvider>
            <CustomCssProvider>
              <TooltipProvider>
                <WindowReveal />
                <MutationDegradationNotifier />
                <DeepLinkImport />
                <Outlet />
              </TooltipProvider>
            </CustomCssProvider>
          </ExperimentalThemeProvider>

          <TanStackRouterDevtools />
        </LanguageProvider>
      </BlockTaskProvider>
    </NyanpasuProvider>
  )
}
