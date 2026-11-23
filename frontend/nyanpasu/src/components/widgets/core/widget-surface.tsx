import {
  createContext,
  use,
  useCallback,
  useEffect,
  useState,
  type RefObject,
} from 'react'

export type WidgetScalingMode = 'proportional' | 'list' | 'none'

export type WidgetDensity = 'compact' | 'comfortable' | 'spacious'

export type WidgetAspect = 'wide' | 'square' | 'tall'

export interface WidgetSurfaceContextValue {
  containerRef: RefObject<HTMLDivElement | null>
  scaling: WidgetScalingMode
}

export const WidgetSurfaceContext =
  createContext<WidgetSurfaceContextValue | null>(null)

export function useWidgetSurface(): WidgetSurfaceContextValue {
  const surface = use(WidgetSurfaceContext)

  if (!surface) {
    throw new Error('useWidgetSurface must be used inside Widget')
  }

  return surface
}

const REFERENCE_EDGE = 176
const MIN_SCALE = 0.85
const MAX_SCALE = 1.85

/**
 * Measures the widget root on demand. Mounting this hook is what adds a
 * ResizeObserver; widgets that rely on CSS container queries do not need it.
 */
export function useWidgetSize() {
  const surface = useWidgetSurface()

  const [rect, setRect] = useState({ width: 0, height: 0 })

  useEffect(() => {
    const element = surface.containerRef.current

    if (!element) {
      return
    }

    const measure = () => {
      const width = element.clientWidth
      const height = element.clientHeight

      setRect((previous) =>
        previous.width === width && previous.height === height
          ? previous
          : { width, height },
      )
    }

    measure()

    const observer = new ResizeObserver(measure)
    observer.observe(element)

    return () => observer.disconnect()
  }, [surface.containerRef])

  const minEdge = Math.min(rect.width, rect.height)
  // The short edge bounds the scale, so wide and tall widgets stay readable.
  const scale =
    minEdge > 0
      ? Math.min(MAX_SCALE, Math.max(MIN_SCALE, minEdge / REFERENCE_EDGE))
      : 1
  const density: WidgetDensity =
    minEdge < 185 ? 'compact' : minEdge < 300 ? 'comfortable' : 'spacious'
  const ratio = rect.height > 0 ? rect.width / rect.height : 1
  const aspect: WidgetAspect =
    ratio > 1.35 ? 'wide' : ratio < 0.75 ? 'tall' : 'square'

  return { ...rect, scale, density, aspect }
}

/** Resolves the container-query row token to pixels without a new API. */
function measureRowHeight(listElement: HTMLElement): number {
  const probe = document.createElement('div')
  probe.style.position = 'absolute'
  probe.style.visibility = 'hidden'
  probe.style.pointerEvents = 'none'
  probe.style.width = '1px'
  probe.style.height = 'var(--w-row-height)'
  listElement.append(probe)
  // clientHeight is layout pixels and ignores the preview stage's transform.
  const height = probe.clientHeight
  probe.remove()

  return height > 0 ? height : 48
}

export interface WidgetListCapacityOptions {
  baseRowHeight?: number
  baseGap?: number
  reserveHeight?: number
  minItems?: number
}

/**
 * Calculates how many list rows fit without scrolling. The row height can
 * come from the container-query `--w-row-height` token, so capacity follows
 * the widget's rendered scale instead of a fixed row constant.
 */
export function useWidgetListCapacity({
  baseRowHeight,
  baseGap = 8,
  reserveHeight = 0,
  minItems = 1,
}: WidgetListCapacityOptions) {
  const [listElement, setListElement] = useState<HTMLElement | null>(null)
  const [capacity, setCapacity] = useState(minItems)

  const listRef = useCallback((node: HTMLElement | null) => {
    setListElement(node)
  }, [])

  useEffect(() => {
    if (!listElement) {
      return
    }

    const compute = () => {
      const available = listElement.clientHeight - reserveHeight

      if (available <= 0) {
        setCapacity(minItems)
        return
      }

      const rowHeight = baseRowHeight ?? measureRowHeight(listElement)
      const fit = Math.floor((available + baseGap) / (rowHeight + baseGap))

      setCapacity(Math.max(minItems, fit))
    }

    compute()

    const observer = new ResizeObserver(compute)
    observer.observe(listElement)

    return () => observer.disconnect()
  }, [listElement, baseRowHeight, baseGap, reserveHeight, minItems])

  return { listRef, capacity }
}
