import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type RefObject,
} from 'react'
import {
  BREAKPOINT_ORDER,
  BREAKPOINT_VALUES,
  type Breakpoint,
} from '@nyanpasu/constants'

let breakpointQueries: [Breakpoint, MediaQueryList][] | null = null

const getBreakpointQueries = (): [Breakpoint, MediaQueryList][] =>
  (breakpointQueries ??= BREAKPOINT_ORDER.slice(1).map(
    (breakpoint) =>
      [
        breakpoint,
        window.matchMedia(`(min-width: ${BREAKPOINT_VALUES[breakpoint]}px)`),
      ] as [Breakpoint, MediaQueryList],
  ))

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
  for (let i = BREAKPOINT_ORDER.length - 1; i >= 0; i--) {
    const breakpoint = BREAKPOINT_ORDER[i]

    if (width >= BREAKPOINT_VALUES[breakpoint]) {
      return breakpoint
    }
  }

  return 'xs'
}

const resolveBreakpointValue = <T>(
  breakpoint: Breakpoint,
  values: BreakpointValues<T>,
  defaultValue?: T,
): T => {
  const value = values[breakpoint]

  if (value !== undefined) {
    return value
  }

  const currentIndex = BREAKPOINT_ORDER.indexOf(breakpoint)

  for (let i = currentIndex; i >= 0; i--) {
    const fallbackValue = values[BREAKPOINT_ORDER[i]]

    if (fallbackValue !== undefined) {
      return fallbackValue
    }
  }

  return defaultValue ?? (values[BREAKPOINT_ORDER[0]] as T)
}

const useContainerBreakpoint = (
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

    return () => resizeObserver.disconnect()
  }, [containerRef])

  return breakpoint
}

export const useContainerBreakpointValue = <T>(
  containerRef: RefObject<HTMLElement | null>,
  values: BreakpointValues<T>,
  defaultValue?: T,
): T => {
  const currentBreakpoint = useContainerBreakpoint(containerRef)

  return useMemo(
    () => resolveBreakpointValue(currentBreakpoint, values, defaultValue),
    [currentBreakpoint, values, defaultValue],
  )
}

export function useIsMobile() {
  const breakpoint = useBreakpoint()

  return breakpoint === 'sm' || breakpoint === 'xs'
}

export function useIsMobileOrTablet() {
  const breakpoint = useBreakpoint()

  return breakpoint === 'sm' || breakpoint === 'xs' || breakpoint === 'md'
}

export type LockFn<P extends unknown[] = unknown[], T = unknown> = (
  ...args: P
) => Promise<T>

export function useLockFn<P extends unknown[] = unknown[], T = unknown>(
  fn: LockFn<P, T>,
): LockFn<P, T> {
  const lockRef = useRef(false)
  const fnRef = useRef(fn)
  fnRef.current = fn

  return useCallback(async (...args: P): Promise<T> => {
    if (lockRef.current) {
      console.warn(`Function is locked: ${fnRef.current.name}`)
      return undefined as T
    }

    lockRef.current = true

    try {
      return await fnRef.current(...args)
    } finally {
      lockRef.current = false
    }
  }, [])
}
