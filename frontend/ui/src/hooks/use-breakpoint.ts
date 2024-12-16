import { useAsyncEffect } from 'ahooks'
import { useEffect, useState } from 'react'
import { createBreakpoint } from 'react-use'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { MUI_BREAKPOINTS } from '../materialYou/themeConsts.mjs'

export type Breakpoint = 'xs' | 'sm' | 'md' | 'lg' | 'xl'

const breakpointsOrder: Breakpoint[] = ['xs', 'sm', 'md', 'lg', 'xl']

export const useBreakpoint = createBreakpoint(
  MUI_BREAKPOINTS.values,
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
      setResult(calculateValue)
    }
  }, [currentBreakpoint, values, defaultValue])

  return result
}
