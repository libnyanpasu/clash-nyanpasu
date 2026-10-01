import {
  RefObject,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
} from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

const appWindow = isTauri() ? getCurrentWebviewWindow() : null

export type Breakpoint = 'xs' | 'sm' | 'md' | 'lg' | 'xl'

export const BREAKPOINT_VALUES: Record<Breakpoint, number> = {
  xs: 0,
  sm: 600,
  md: 900,
  lg: 1200,
  xl: 1536,
}

const breakpointsOrder: Breakpoint[] = ['xs', 'sm', 'md', 'lg', 'xl']

// One media query per breakpoint above `xs`, created on first use. A query
// only reports crossing its threshold, so resizing within a breakpoint
// re-renders nothing, unlike tracking `window.innerWidth`.
let breakpointQueries: [Breakpoint, MediaQueryList][] | null = null

const getBreakpointQueries = () =>
  (breakpointQueries ??= breakpointsOrder
    .slice(1)
    .map((breakpoint) => [
      breakpoint,
      window.matchMedia(`(min-width: ${BREAKPOINT_VALUES[breakpoint]}px)`),
    ]))

const subscribeBreakpoint = (onChange: () => void) => {
  const queries = getBreakpointQueries()

  queries.forEach(([, query]) => query.addEventListener('change', onChange))

  return () => {
    queries.forEach(([, query]) =>
      query.removeEventListener('change', onChange),
    )
  }
}

const getBreakpoint = (): Breakpoint => {
  let current: Breakpoint = 'xs'

  for (const [breakpoint, query] of getBreakpointQueries()) {
    if (query.matches) {
      current = breakpoint
    }
  }

  return current
}

export const useBreakpoint = (): Breakpoint =>
  useSyncExternalStore(subscribeBreakpoint, getBreakpoint)

type BreakpointValues<T> = Partial<Record<Breakpoint, T>>

const getBreakpointFromWidth = (width: number): Breakpoint => {
  for (let i = breakpointsOrder.length - 1; i >= 0; i--) {
    const breakpoint = breakpointsOrder[i]

    if (width >= BREAKPOINT_VALUES[breakpoint]) {
      return breakpoint
    }
  }

  return 'xs'
}

export const useBreakpointValue = <T>(
  values: BreakpointValues<T>,
  defaultValue?: T,
): T => {
  const currentBreakpoint = useBreakpoint()

  const calculateValue = (): T => {
    const value = values[currentBreakpoint]

    if (value !== undefined) {
      return value as T
    }

    const currentIndex = breakpointsOrder.indexOf(currentBreakpoint)

    for (let i = currentIndex; i >= 0; i--) {
      const fallbackValue = values[breakpointsOrder[i]]

      if (fallbackValue !== undefined) {
        return fallbackValue as T
      }
    }

    return defaultValue ?? (values[breakpointsOrder[0]] as T)
  }

  const [result, setResult] = useState<T>(calculateValue)

  useEffect(() => {
    let cancelled = false

    const minimized = appWindow
      ? appWindow.isMinimized()
      : Promise.resolve(false)
    minimized.then((isMinimized) => {
      if (cancelled || isMinimized) {
        return
      }

      const nextValue = calculateValue()

      if (result !== nextValue) {
        setResult(nextValue)
      }
    })

    return () => {
      cancelled = true
    }
    // oxlint-disable-next-line eslint-plugin-react-hooks/exhaustive-deps
  }, [currentBreakpoint, values, defaultValue])

  return result
}

export const useContainerBreakpoint = (
  containerRef: RefObject<HTMLElement | null>,
): Breakpoint => {
  const [breakpoint, setBreakpoint] = useState<Breakpoint>(() => {
    if (containerRef.current) {
      return getBreakpointFromWidth(containerRef.current.offsetWidth)
    }

    return 'md'
  })

  useEffect(() => {
    const element = containerRef.current

    if (!element) {
      return
    }

    const resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setBreakpoint(getBreakpointFromWidth(entry.contentRect.width))
      }
    })

    resizeObserver.observe(element)

    return () => {
      resizeObserver.disconnect()
    }
  }, [containerRef])

  return breakpoint
}

export const useContainerBreakpointValue = <T>(
  containerRef: RefObject<HTMLElement | null>,
  values: BreakpointValues<T>,
  defaultValue?: T,
): T => {
  const currentBreakpoint = useContainerBreakpoint(containerRef)

  return useMemo(() => {
    const value = values[currentBreakpoint]

    if (value !== undefined) {
      return value as T
    }

    const currentIndex = breakpointsOrder.indexOf(currentBreakpoint)

    for (let i = currentIndex; i >= 0; i--) {
      const fallbackValue = values[breakpointsOrder[i]]

      if (fallbackValue !== undefined) {
        return fallbackValue as T
      }
    }

    return defaultValue ?? (values[breakpointsOrder[0]] as T)
  }, [currentBreakpoint, values, defaultValue])
}
