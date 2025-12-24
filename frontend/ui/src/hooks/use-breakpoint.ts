import { useAsyncEffect } from 'ahooks'
import { RefObject, useEffect, useMemo, useState } from 'react'
import { createBreakpoint } from 'react-use'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { MUI_BREAKPOINTS } from '../materialYou/themeConsts.mjs'

export type Breakpoint = 'xs' | 'sm' | 'md' | 'lg' | 'xl'

const breakpointsOrder: Breakpoint[] = ['xs', 'sm', 'md', 'lg', 'xl']

const BREAKPOINT_VALUES = MUI_BREAKPOINTS.values as Record<Breakpoint, number>

export const useBreakpoint = createBreakpoint(
  BREAKPOINT_VALUES,
) as () => Breakpoint

type BreakpointEffectCallback = (currentBreakpoint: Breakpoint) => void

export const useBreakpointEffect = (callback: BreakpointEffectCallback) => {
  const currentBreakpoint = useBreakpoint()

  useEffect(() => {
    callback(currentBreakpoint)
  }, [currentBreakpoint, callback])
}

type BreakpointValues<T> = Partial<Record<Breakpoint, T>>

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

  useAsyncEffect(async () => {
    const appWindow = getCurrentWebviewWindow()
    if (!(await appWindow.isMinimized())) {
      if (result !== calculateValue) {
        setResult(calculateValue)
      }
    }
  }, [currentBreakpoint, values, defaultValue])

  return result
}

const getBreakpointFromWidth = (width: number): Breakpoint => {
  for (let i = breakpointsOrder.length - 1; i >= 0; i--) {
    const bp = breakpointsOrder[i]
    if (width >= BREAKPOINT_VALUES[bp]) {
      return bp
    }
  }
  return 'xs'
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
        const width = entry.contentRect.width
        const newBreakpoint = getBreakpointFromWidth(width)
        setBreakpoint(newBreakpoint)
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

  const memoizedValue = useMemo(
    () => calculateValue(),
    [currentBreakpoint, values, defaultValue],
  )

  return memoizedValue
}
