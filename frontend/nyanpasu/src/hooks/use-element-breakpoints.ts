import { RefObject, useEffect, useState } from 'react'

export const useElementBreakpoints = (
  element: RefObject<HTMLElement>,
  breakpoints: { [key: string]: number },
  defaultBreakpoint: string,
) => {
  const [breakpoint, setBreakpoint] = useState<string | null>(defaultBreakpoint)

  useEffect(() => {
    let observer: ResizeObserver | null = null
    if (element.current) {
      observer = new ResizeObserver(() => {
        const { width } = element.current.getBoundingClientRect()
        const breakpoint = Object.entries(breakpoints).find(
          ([, value]) => width >= value,
        )?.[0]
        if (breakpoint) {
          setBreakpoint(breakpoint)
        }
      })
      observer.observe(element.current)
    }
    return () => observer?.disconnect()
  }, [element, breakpoints])

  return breakpoint
}
