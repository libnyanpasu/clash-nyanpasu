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

    const measure = () => setHeight(element.getBoundingClientRect().height)
    measure()

    const observer = new ResizeObserver(measure)
    observer.observe(element)

    return () => {
      observer.disconnect()
    }
  }, [element])

  return { ref, height }
}
