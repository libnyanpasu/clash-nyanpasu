import {
  AnimatePresence,
  clamp,
  cubicBezier,
  domAnimation,
  LazyMotion,
  motion,
} from 'motion/react'
import {
  useCallback,
  useEffect,
  useEffectEvent,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type Key,
  type MouseEvent,
  type PointerEvent,
} from 'react'
import {
  getRippleRenderer,
  resolveRippleColor,
  type RippleColor,
  type RippleFrame,
} from './ripple-shader'

export type RippleConfig = {
  key: Key
  /** Top-left corner of the `size` square centered on the touch point. */
  x: number
  y: number
  size: number
  /** `performance.now()` when the press started. */
  startedAt: number
  /** `performance.now()` when the press ended; unset while it is held. */
  releasedAt?: number
}

export interface RippleProps {
  ripples: RippleConfig[]
  color?: string
  onClear: (key: Key) => void
}

/** AOSP `RippleAnimationSession` enter and exit durations. */
export const RIPPLE_ENTER_DURATION = 450
export const RIPPLE_EXIT_DURATION = 375

/** Wave alpha; the soft edge needs more than the 12% state layer to read. */
const RIPPLE_OPACITY = 0.2
/** AOSP sparkle alpha `0x8d`; white reads only on dark surfaces. */
const RIPPLE_SPARKLE_ALPHA = 0x8d / 255
const RIPPLE_SPARKLE_LIGHT: RippleColor = [1, 1, 1, RIPPLE_SPARKLE_ALPHA]
const RIPPLE_SPARKLE_DARK: RippleColor = [0, 0, 0, RIPPLE_SPARKLE_ALPHA]

/** AOSP `FAST_OUT_SLOW_IN` enter interpolator. */
const rippleEnterEasing = cubicBezier(0.4, 0, 0.2, 1)

/**
 * Shader progress for a ripple: it enters to 0.5 and holds there while
 * pressed. The exit to 1 starts on release, but never before the enter
 * finishes, so a quick tap still shows the whole wave.
 */
export function rippleProgress(
  now: number,
  startedAt: number,
  releasedAt?: number,
): number {
  const enter = rippleEnterEasing(
    clamp(0, 1, (now - startedAt) / RIPPLE_ENTER_DURATION),
  )

  if (releasedAt == null) {
    return enter * 0.5
  }

  const exitStart = Math.max(releasedAt, startedAt + RIPPLE_ENTER_DURATION)
  const exit = clamp(0, 1, (now - exitStart) / RIPPLE_EXIT_DURATION)

  return enter * 0.5 + exit * 0.5
}

/**
 * Renders the A17 patterned ripple through WebGL, falling back to the scaled
 * ink circle when WebGL is unavailable or its context is lost.
 */
export const Ripple = (props: RippleProps) => {
  if (props.ripples.length === 0) {
    return null
  }

  return <ShaderRipple {...props} />
}

const rippleCanvasStyle: CSSProperties = {
  position: 'absolute',
  inset: 0,
  zIndex: 0,
  width: '100%',
  height: '100%',
  pointerEvents: 'none',
}

const ShaderRipple = ({ ripples, color, onClear }: RippleProps) => {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [fallback, setFallback] = useState(() => getRippleRenderer() == null)

  const clear = useEffectEvent((key: Key) => onClear(key))

  // A layout effect draws the first frame before the press paints.
  useLayoutEffect(() => {
    const canvas = canvasRef.current
    const context = canvas?.getContext('2d')
    const renderer = getRippleRenderer()

    if (fallback || !canvas || !context || !renderer) {
      if (!fallback) {
        setFallback(true)
      }
      return
    }

    const [r, g, b, a] = resolveRippleColor(
      color ?? getComputedStyle(canvas).color,
    )
    const rippleColor: RippleColor = [r, g, b, a * RIPPLE_OPACITY]
    // The ripple takes the host's text color, so dark text means a light
    // surface, where dark sparkles stay visible.
    const onLightSurface = 0.2126 * r + 0.7152 * g + 0.0722 * b < 0.5
    const sparkleColor = onLightSurface
      ? RIPPLE_SPARKLE_DARK
      : RIPPLE_SPARKLE_LIGHT
    // A dark tint on a light surface has about half the contrast of a light
    // tint on a dark one, so its tail would vanish early; ease it out instead.
    const easeFadeOut = onLightSurface
    let frameId = 0

    const draw = (now: number) => {
      const frames: RippleFrame[] = []

      for (const ripple of ripples) {
        const progress = rippleProgress(
          now,
          ripple.startedAt,
          ripple.releasedAt,
        )

        if (progress >= 1) {
          clear(ripple.key)
          continue
        }

        frames.push({
          x: ripple.x + ripple.size / 2,
          y: ripple.y + ripple.size / 2,
          progress,
          startedAt: ripple.startedAt,
          elapsed: now - ripple.startedAt,
          color: rippleColor,
          sparkleColor,
          easeFadeOut,
        })
      }

      const rendered = renderer.render(
        {
          context,
          width: canvas.clientWidth,
          height: canvas.clientHeight,
          pixelRatio: window.devicePixelRatio || 1,
        },
        frames,
      )

      if (!rendered) {
        setFallback(true)
        return
      }

      if (frames.length > 0) {
        frameId = requestAnimationFrame(draw)
      }
    }

    draw(performance.now())

    return () => cancelAnimationFrame(frameId)
  }, [ripples, color, fallback])

  if (fallback) {
    return <MotionRipple ripples={ripples} color={color} onClear={onClear} />
  }

  return (
    <canvas
      ref={canvasRef}
      aria-hidden
      // Inline so the canvas never takes its default 300x150 box or catches
      // the pointer, even before (or without) the stylesheet.
      style={rippleCanvasStyle}
      data-slot="ripple-canvas"
    />
  )
}

const MotionRipple = ({ ripples, color, onClear }: RippleProps) => {
  return ripples.map((ripple) => {
    const duration = clamp(
      ripple.size > 100 ? 0.6 : 0.4,
      0.01 * ripple.size,
      0.3,
    )

    return (
      <LazyMotion key={ripple.key} features={domAnimation}>
        <AnimatePresence mode="popLayout">
          <motion.span
            className="pointer-events-none absolute inset-0 z-0 origin-center overflow-hidden rounded-full"
            data-slot="ripple-item"
            initial={{ transform: 'scale(0)', opacity: 0.4 }}
            animate={{ transform: 'scale(2)', opacity: 0 }}
            exit={{ opacity: 0 }}
            transition={{ duration }}
            style={{
              backgroundColor: color ?? 'currentColor',
              top: ripple.y,
              left: ripple.x,
              width: `${ripple.size}px`,
              height: `${ripple.size}px`,
            }}
            onAnimationComplete={() => {
              onClear(ripple.key)
            }}
          />
        </AnimatePresence>
      </LazyMotion>
    )
  })
}

export interface UseRippleOptions {
  /**
   * Element whose bounds place the ripple; defaults to the event target.
   * A press outside these bounds starts the ripple from their center.
   */
  getHost?: () => Element | null
}

/**
 * Wire `onPointerDown` to start a ripple that holds until the pointer is
 * released. Wire `onClick` to ripple from the center on keyboard or
 * programmatic clicks; pointer clicks are already covered by the press.
 */
export const useRipple = ({ getHost }: UseRippleOptions = {}) => {
  const [ripples, setRipples] = useState<RippleConfig[]>([])
  const nextKey = useRef(0)
  const releaseActive = useRef<(() => void) | null>(null)

  useEffect(() => () => releaseActive.current?.(), [])

  const spawn = useCallback(
    (event: MouseEvent, released: boolean) => {
      const host = getHost?.() ?? event.currentTarget
      const rect = host.getBoundingClientRect()
      const fromPointer =
        !released &&
        event.clientX >= rect.left &&
        event.clientX <= rect.right &&
        event.clientY >= rect.top &&
        event.clientY <= rect.bottom
      const size = Math.max(rect.width, rect.height)
      const x = fromPointer ? event.clientX - rect.left : rect.width / 2
      const y = fromPointer ? event.clientY - rect.top : rect.height / 2
      const key = ++nextKey.current
      const now = performance.now()

      setRipples((prev) => [
        ...prev,
        {
          key,
          size,
          x: x - size / 2,
          y: y - size / 2,
          startedAt: now,
          releasedAt: released ? now : undefined,
        },
      ])

      return key
    },
    [getHost],
  )

  const onPointerDown = useCallback(
    (event: PointerEvent) => {
      if (event.button !== 0) {
        return
      }

      releaseActive.current?.()

      const key = spawn(event, false)
      const listeners = new AbortController()
      const release = () => {
        listeners.abort()
        releaseActive.current = null
        setRipples((prev) =>
          prev.map((ripple) =>
            ripple.key === key && ripple.releasedAt == null
              ? { ...ripple, releasedAt: performance.now() }
              : ripple,
          ),
        )
      }

      releaseActive.current = release
      window.addEventListener('pointerup', release, {
        signal: listeners.signal,
      })
      window.addEventListener('pointercancel', release, {
        signal: listeners.signal,
      })
    },
    [spawn],
  )

  const onClick = useCallback(
    (event: MouseEvent) => {
      if (event.detail === 0) {
        spawn(event, true)
      }
    },
    [spawn],
  )

  const onClear = useCallback((key: Key) => {
    setRipples((prev) => prev.filter((ripple) => ripple.key !== key))
  }, [])

  const clearAll = useCallback(() => {
    releaseActive.current?.()
    setRipples([])
  }, [])

  return { ripples, onPointerDown, onClick, onClear, clearAll }
}
