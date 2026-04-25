import { RefObject, useEffect, useMemo, useState } from 'react'
import createBreakpoint from 'react-use/esm/factory/createBreakpoint'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

const appWindow = getCurrentWebviewWindow()

export type Breakpoint = 'xs' | 'sm' | 'md' | 'lg' | 'xl'

export const BREAKPOINT_VALUES: Record<Breakpoint, number> = {
  xs: 0,
  sm: 600,
  md: 900,
  lg: 1200,
  xl: 1536,
}

const breakpointsOrder: Breakpoint[] = ['xs', 'sm', 'md', 'lg', 'xl']

export const useBreakpoint = createBreakpoint(
  BREAKPOINT_VALUES,
) as () => Breakpoint

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

    appWindow.isMinimized().then((isMinimized) => {
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
