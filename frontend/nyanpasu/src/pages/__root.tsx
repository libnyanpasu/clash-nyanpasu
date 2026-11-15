import {
  lazy,
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from 'react'
import { SnackbarProvider } from '@nyanpasu/ui/snackbar'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { AuthBootstrapGate } from '@/components/auth/auth-bootstrap-gate'
import { useCapabilities } from '@/components/auth/capabilities'
import { ImportUrlProvider } from '@/components/profile-import/import-url-provider'
import { useProfileLookup } from '@/components/profile-label'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import CustomCssProvider from '@/components/providers/custom-css-provider'
import { LanguageProvider } from '@/components/providers/language-provider'
import { ExperimentalThemeProvider } from '@/components/providers/theme-provider'
import { useDeepLinkImport } from '@/hooks/use-deep-link-import'
import { m } from '@/paraglide/messages'
import { queryClient, rpc } from '@/services/rpc'
import { formatError } from '@/utils'
import {
  degradationReasonMessage,
  publicDegradationReasonMessage,
} from '@/utils/ipc-error'
import { message } from '@/utils/notification'
import { profileDialogLabel, type ProfileLabel } from '@/utils/profile-label'
import {
  createWindowAdapter,
  detectRuntime,
  type RuntimeInfo,
} from '@nyanpasu/platform'
import { PlatformProvider } from '@nyanpasu/platform/react'
import {
  NyanpasuQueryProvider,
  useSettings,
  type AnyDegradation,
} from '@nyanpasu/query'
import { unwrapResult } from '@nyanpasu/rpc'
import { type DegradationPhase } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import {
  createRootRouteWithContext,
  ErrorComponentProps,
  Outlet,
  redirect,
} from '@tanstack/react-router'

// The backend shows a window once its frontend has rendered.
const reportWindowReady = () => {
  if (detectRuntime().bridge === 'tauri') {
    rpc
      .reportWindowReady()
      .then(unwrapResult)
      .catch((error) =>
        console.error('failed to report the window ready', error),
      )
  }
}

type RootContext = { runtime?: RuntimeInfo }

function RootPlatformBoundary({ children }: { children: ReactNode }) {
  const contextRuntime = Route.useRouteContext({
    select: (context) => context.runtime,
  })
  const [runtime] = useState(() => contextRuntime ?? detectRuntime())
  return (
    <PlatformProvider platform={runtime.platform} bridge={runtime.bridge}>
      {children}
    </PlatformProvider>
  )
}

export const Catch = ({ error }: ErrorComponentProps) => {
  const [appWindow] = useState(createWindowAdapter)
  // A window that failed to render is still shown, with the error.
  useEffect(reportWindowReady, [])

  return (
    <RootPlatformBoundary>
      <div
        className={cn('h-dvh bg-black text-white', 'flex flex-col gap-4 p-4')}
      >
        <div
          className="fixed top-0 left-0 z-10 h-6 w-full"
          data-tauri-drag-region
        />

        <h1 data-tauri-drag-region>Oops!</h1>

        <p>Something went wrong... Caught in error boundary.</p>

        <pre className="overflow-x-auto font-mono whitespace-pre-wrap select-text">
          {error instanceof Error ? (
            <>
              {error.message}
              {error.stack}
            </>
          ) : (
            formatError(error)
          )}
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
            onClick={() => appWindow?.close()}
          >
            Close Window
          </button>
        </div>
      </div>
    </RootPlatformBoundary>
  )
}

export const Pending = () => (
  <RootPlatformBoundary>
    <div>Loading from _root...</div>
  </RootPlatformBoundary>
)

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

export const Route = createRootRouteWithContext<RootContext>()({
  beforeLoad: ({ context, location }) => {
    if (
      (context.runtime ?? detectRuntime()).platform.surface === 'browser' &&
      location.pathname === '/'
    ) {
      throw redirect({ to: '/main/dashboard' })
    }
  },
  component: App,
  errorComponent: Catch,
  pendingComponent: Pending,
})

function WindowReadyReporter({ canRead }: { canRead: boolean }) {
  const { query } = useSettings({ enabled: canRead })
  const hasReported = useRef(false)

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
    if (
      (!canRead || query.isSuccess || query.isError) &&
      !hasReported.current
    ) {
      hasReported.current = true
      reportWindowReady()
    }
  }, [canRead, query.isSuccess, query.isError])

  return null
}

function localizeDegradationPhase(phase: DegradationPhase): string {
  switch (phase) {
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

function formatDegradationItem(
  degradation: AnyDegradation,
  profileLabel: ProfileLabel,
): string {
  const detail =
    'message' in degradation
      ? degradationReasonMessage(degradation.reason, profileLabel)
      : publicDegradationReasonMessage(degradation.reason)
  return m.mutation_degraded_item({
    phase: localizeDegradationPhase(degradation.phase),
    detail,
  })
}

type DegradationHandler = (degradations: AnyDegradation[]) => void

function MutationDegradationNotifier({
  handler,
  canRead,
}: {
  handler: RefObject<DegradationHandler | null>
  canRead: boolean
}) {
  const profiles = useProfileLookup({ enabled: canRead })
  useEffect(() => {
    const notify: DegradationHandler = (degradations) => {
      if (degradations.length === 0) {
        return
      }

      // Backend `message` is diagnostic-only; primary copy is phase + reason.
      for (const degradation of degradations) {
        console.warn('[mutation-degradation]', {
          phase: degradation.phase,
          reason: degradation.reason,
          retryable: degradation.retryable,
          message: 'message' in degradation ? degradation.message : undefined,
        })
      }

      const items = degradations
        .map((degradation) =>
          formatDegradationItem(degradation, (id) =>
            profileDialogLabel(profiles, id),
          ),
        )
        .join('; ')
      message(m.mutation_degraded_summary({ items }), {
        title: m.mutation_degraded_title(),
        kind: 'warning',
      }).catch((error) => {
        console.error('[mutation-degradation] failed to show warning', error)
      })
    }
    handler.current = notify
    return () => {
      if (handler.current === notify) handler.current = null
    }
  }, [handler, profiles])
  return null
}

function DeepLinkImport() {
  useDeepLinkImport()
  return null
}

function BootstrapErrorFallback({
  error,
  retry,
}: {
  error: unknown
  retry: () => void
}) {
  useEffect(reportWindowReady, [])

  return (
    <div className={cn('flex h-dvh flex-col gap-4 bg-black p-4 text-white')}>
      <div
        className="fixed top-0 left-0 z-10 h-6 w-full"
        data-tauri-drag-region
      />
      <h1 data-tauri-drag-region>Connection error</h1>
      <pre className="overflow-x-auto font-mono whitespace-pre-wrap select-text">
        {formatError(error)}
      </pre>
      <div>
        <button
          type="button"
          className="cursor-pointer bg-zinc-900 px-3 py-2 text-zinc-100"
          onClick={retry}
        >
          Retry
        </button>
      </div>
    </div>
  )
}

function AuthenticatedApp() {
  const [appWindow] = useState(createWindowAdapter)
  const { hasScope } = useCapabilities()
  const canRead = hasScope('read')
  const canSensitiveRead = hasScope('sensitive_read')
  const degradationHandler = useRef<DegradationHandler | null>(null)
  const onDegraded = useCallback(
    (items: AnyDegradation[]) => degradationHandler.current?.(items),
    [],
  )

  const appTree = (
    <TooltipProvider>
      <SnackbarProvider>
        <ImportUrlProvider>
          <WindowReadyReporter canRead={canRead} />
          <MutationDegradationNotifier
            handler={degradationHandler}
            canRead={canRead}
          />
          {appWindow?.label() === 'main' && <DeepLinkImport />}
          <Outlet />
        </ImportUrlProvider>
      </SnackbarProvider>
    </TooltipProvider>
  )

  return (
    <NyanpasuQueryProvider
      rpc={rpc}
      queryClient={queryClient}
      onDegraded={onDegraded}
      kvPersistence={canSensitiveRead ? 'backend' : 'local'}
      clashWsEnabled={canRead}
    >
      <BlockTaskProvider>
        <LanguageProvider enabled={canRead}>
          <ExperimentalThemeProvider enabled={canRead}>
            {canSensitiveRead ? (
              <CustomCssProvider>{appTree}</CustomCssProvider>
            ) : (
              appTree
            )}
          </ExperimentalThemeProvider>

          <TanStackRouterDevtools />
        </LanguageProvider>
      </BlockTaskProvider>
    </NyanpasuQueryProvider>
  )
}

export default function App() {
  return (
    <RootPlatformBoundary>
      <AuthBootstrapGate
        describe={() => rpc.rpcDescribe()}
        queryClient={queryClient}
        renderError={(error, retry) => (
          <BootstrapErrorFallback error={error} retry={retry} />
        )}
      >
        <AuthenticatedApp />
      </AuthBootstrapGate>
    </RootPlatformBoundary>
  )
}
