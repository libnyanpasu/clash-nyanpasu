import {
  createContext,
  createElement,
  Fragment,
  useContext,
  useMemo,
  type ReactNode,
} from 'react'
import {
  detectRuntime,
  matchesPlatform,
  type Bridge,
  type PlatformCondition,
  type PlatformInfo,
  type RuntimeInfo,
} from './index'

const PlatformContext = createContext<RuntimeInfo | null>(null)

export function PlatformProvider({
  platform,
  bridge = 'none',
  children,
}: {
  platform: PlatformInfo
  bridge?: Bridge
  children: ReactNode
}) {
  return createElement(
    PlatformContext.Provider,
    { value: { platform, bridge } },
    children,
  )
}

export function useRuntime(): RuntimeInfo {
  const runtime = useContext(PlatformContext)
  const detected = useMemo(detectRuntime, [])
  // The application installs a stable snapshot above the router. The runtime
  // fallback also keeps standalone components and test harnesses usable.
  return runtime ?? detected
}

export function usePlatform(): PlatformInfo {
  return useRuntime().platform
}

export function useMatchesPlatform(when: PlatformCondition): boolean {
  return matchesPlatform(usePlatform(), when)
}

export function Platform({
  when,
  fallback = null,
  children,
}: {
  when: PlatformCondition
  fallback?: ReactNode
  children?: ReactNode
}) {
  return useMatchesPlatform(when)
    ? createElement(Fragment, null, children)
    : fallback
}

export function PlatformDesktop(
  props: Omit<Parameters<typeof Platform>[0], 'when'>,
) {
  return <Platform when="desktop" {...props} />
}
export function PlatformBrowser(
  props: Omit<Parameters<typeof Platform>[0], 'when'>,
) {
  return <Platform when="browser" {...props} />
}
export function PlatformWindows(
  props: Omit<Parameters<typeof Platform>[0], 'when'>,
) {
  return <Platform when="windows" {...props} />
}
export function PlatformMacos(
  props: Omit<Parameters<typeof Platform>[0], 'when'>,
) {
  return <Platform when="macos" {...props} />
}
export function PlatformLinux(
  props: Omit<Parameters<typeof Platform>[0], 'when'>,
) {
  return <Platform when="linux" {...props} />
}
