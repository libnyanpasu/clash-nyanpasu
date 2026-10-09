import { useCallback, useEffect, useState } from 'react'

export function useWidgetHeight<T extends HTMLElement>() {
  const [element, setElement] = useState<T | null>(null)
  const [height, setHeight] = useState<number | null>(null)
  const ref = useCallback((node: T | null) => {
    setElement(node)
    if (!node) setHeight(null)
  }, [])

  useEffect(() => {
    if (!element) return

    // Layout capacity must not shrink with the widget's entrance/drag transform.
    const measure = () => setHeight(element.clientHeight)
    measure()

    const observer = new ResizeObserver(measure)
    observer.observe(element)

    return () => {
      observer.disconnect()
    }
  }, [element])

  return { ref, height }
}
