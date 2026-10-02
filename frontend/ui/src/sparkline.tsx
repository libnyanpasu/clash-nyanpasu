import * as d3 from 'd3'
import { isEqual } from 'es-toolkit'
import { ComponentPropsWithoutRef, useEffect, useRef } from 'react'
import { cn } from '@nyanpasu/utils'

/**
 * Coefficient of variation threshold (std / mean) below which the series is
 * considered "stable".  CV is scale-independent: 1–2 and 9 000–11 000 are
 * evaluated on the same relative basis regardless of their absolute magnitude.
 */
const STABLE_CV_THRESHOLD = 0.15

/**
 * When the series is stable, the chart only occupies the bottom third of the
 * SVG height (topFactor = 2/3 — usable band = height - height*(2/3) = h/3).
 */
const STABLE_TOP_FACTOR = 2 / 3

/** When the series has a wide range, use most of the available height. */
const ACTIVE_TOP_FACTOR = 0.35

/** Vertical mapping of the chart: y = 0 at the bottom, y = yMax at topFactor. */
type Scale = { yMax: number; topFactor: number }

const computeScale = (points: number[]): Scale => {
  const mean = d3.mean(points) ?? 0
  const std = d3.deviation(points) ?? 0
  const cv = mean > 0 ? std / mean : 0

  return {
    yMax: Math.max(d3.max(points) ?? 0, 1),
    topFactor: cv < STABLE_CV_THRESHOLD ? STABLE_TOP_FACTOR : ACTIVE_TOP_FACTOR,
  }
}

/**
 * Number of samples `next` has advanced past `prev` in a fixed-size sliding
 * window, or 0 when `next` is not a continuation of `prev`.
 */
const findShift = (prev: number[], next: number[]) => {
  if (prev.length !== next.length) {
    return 0
  }

  for (let k = 1; k < next.length; k++) {
    if (next.slice(0, next.length - k).every((v, i) => v === prev[i + k])) {
      return k
    }
  }

  return 0
}

/** Vertical pixels per data unit, relative to the chart height. */
const yDensity = ({ yMax, topFactor }: Scale) => (1 - topFactor) / yMax

const buildPaths = (
  points: number[],
  stepWidth: number,
  height: number,
  { yMax, topFactor }: Scale,
  leftGuard?: number,
) => {
  const n = points.length

  // Prepend/append invisible guard points one step outside each edge so that
  // every visible data point is treated as an interior spline node, eliminating
  // the endpoint tangent discontinuity that causes boundary wobble.
  // The root's overflow:hidden clips the guard region.
  const guarded = [
    leftGuard ?? 2 * points[0] - points[1],
    ...points,
    2 * points[n - 1] - points[n - 2],
  ]

  const x = (_: number, i: number) => (i - 1) * stepWidth
  const y = d3
    .scaleLinear()
    .domain([0, yMax])
    .range([height, height * topFactor])
  const curve = d3.curveCatmullRom.alpha(0.5)

  return {
    line:
      d3
        .line<number>()
        .x(x)
        .y((d) => y(d))
        .curve(curve)(guarded) ?? '',
    area:
      d3
        .area<number>()
        .x(x)
        .y0(height)
        .y1((d) => y(d))
        .curve(curve)(guarded) ?? '',
  }
}

/** d3.easeCubicInOut as a CSS easing. */
const RESCALE_EASING = 'cubic-bezier(0.65, 0, 0.35, 1)'

type Chart = {
  // Slides the chart left (linear) and, inside it, stretches it vertically
  // about the bottom edge (eased), so the two axes never couple.
  slide: HTMLDivElement
  stretch: HTMLDivElement
  area: SVGPathElement
  line: SVGPathElement
  // Tracked by a ResizeObserver, so drawing never reads layout.
  size: { width: number; height: number } | null
  data: number[] | null
  // Every point currently drawn; its last N points always equal the latest
  // series, and the ones before them are still scrolling off to the left.
  buf: number[] | null
  // The point just before buf[0], so the curve shape at the left edge stays
  // the same when scrolled-off points are dropped from the buffer.
  leftGuard: number | null
  // The scale the paths are drawn with.
  scale: Scale | null
  slideAnim: { animation: Animation; from: number; to: number } | null
  stretchAnim: { animation: Animation; from: number } | null
}

const progressOf = (animation: Animation) =>
  animation.effect?.getComputedTiming().progress ?? 1

// Scroll position of the buffer in steps, as on screen.
const currentOffset = ({ slideAnim }: Chart) =>
  slideAnim
    ? slideAnim.from +
      (slideAnim.to - slideAnim.from) * progressOf(slideAnim.animation)
    : 0

// Vertical stretch of the drawn paths, as on screen.
const currentStretch = ({ stretchAnim }: Chart) =>
  stretchAnim
    ? stretchAnim.from +
      (1 - stretchAnim.from) * progressOf(stretchAnim.animation)
    : 1

const stopAnimations = (chart: Chart) => {
  chart.slideAnim?.animation.cancel()
  chart.slideAnim = null
  chart.stretchAnim?.animation.cancel()
  chart.stretchAnim = null
}

const drawPaths = (chart: Chart, points: number[], scale: Scale) => {
  const { size, data } = chart
  if (!size || !data) {
    return
  }

  const stepWidth = size.width / (data.length - 1)
  const { line, area } = buildPaths(
    points,
    stepWidth,
    size.height,
    scale,
    chart.leftGuard ?? undefined,
  )

  // Wide enough for the points still scrolling off.
  chart.slide.style.width = `${Math.max(size.width, stepWidth * (points.length - 1))}px`
  chart.area.setAttribute('d', area)
  chart.line.setAttribute('d', line)
}

// Draw the latest series at rest.
const drawSettled = (chart: Chart) => {
  stopAnimations(chart)

  const { data } = chart
  if (!data || data.length < 2) {
    chart.area.removeAttribute('d')
    chart.line.removeAttribute('d')
    chart.buf = null
    chart.leftGuard = null
    chart.scale = null
    return
  }

  chart.buf = data.slice()
  chart.scale = computeScale(data)
  drawPaths(chart, data, chart.scale)
}

// Scroll in the samples `data` has over the previous series. Each sample draws
// the paths once; the scroll and any rescale run as Web Animations on the
// wrappers' transforms, which the compositor runs with no per-frame work on
// the main thread.
const scrollIn = (
  chart: Chart,
  data: number[],
  shift: number,
  duration: number,
) => {
  const { size } = chart
  let buf = chart.buf
  if (!size || !buf) {
    return
  }

  // A new sample may arrive before the previous scroll finished. Continue from
  // the current position instead of snapping to a cycle boundary: drop the
  // points that have fully scrolled off, append the new samples and scroll on.
  const offset = currentOffset(chart)
  const stretch = currentStretch(chart)
  stopAnimations(chart)

  const scrolled = Math.floor(offset)
  if (scrolled > 0) {
    chart.leftGuard = buf[scrolled - 1]
    buf = buf.slice(scrolled)
  }
  buf = [...buf, ...data.slice(-shift)]
  chart.buf = buf

  const fromOffset = offset - scrolled
  const toOffset = buf.length - data.length
  const stepWidth = size.width / (data.length - 1)

  // Draw at the new scale, stretched to the scale actually on screen
  // (possibly mid-transition), so a new peak or a stability change never
  // rescales the chart in one frame.
  const toScale = computeScale(data)
  const fromStretch =
    (stretch * yDensity(chart.scale ?? toScale)) / yDensity(toScale)
  chart.scale = toScale
  drawPaths(chart, buf, toScale)

  if (Math.abs(fromStretch - 1) > 1e-3) {
    const animation = chart.stretch.animate(
      [{ transform: `scaleY(${fromStretch})` }, { transform: 'scaleY(1)' }],
      { duration, easing: RESCALE_EASING },
    )
    chart.stretchAnim = { animation, from: fromStretch }
  }

  // X-axis: pure linear translation — the scroll must feel constant-speed.
  const animation = chart.slide.animate(
    [
      { transform: `translateX(${-stepWidth * fromOffset}px)` },
      { transform: `translateX(${-stepWidth * toOffset}px)` },
    ],
    // Hold the end position until the settled paths replace the buffer.
    { duration, easing: 'linear', fill: 'forwards' },
  )
  chart.slideAnim = { animation, from: fromOffset, to: toOffset }

  animation.onfinish = () => {
    if (chart.slideAnim?.animation !== animation) {
      return
    }

    // The last scrolled-off point becomes the left guard for the next cycle
    // so the curve shape at x=0 stays consistent across animation boundaries.
    // At the end of the scroll the buffer path at -toOffset steps and the
    // N-point path at x=0 occupy identical visual coordinates, so the swap
    // is seamless.
    chart.leftGuard = buf[toOffset - 1]
    drawSettled(chart)
  }
}

export const Sparkline = ({
  data,
  animationDuration = 1,
  className,
  ...props
}: ComponentPropsWithoutRef<'div'> & {
  data: number[]
  animationDuration?: number
}) => {
  const rootRef = useRef<HTMLDivElement | null>(null)
  const slideRef = useRef<HTMLDivElement | null>(null)
  const stretchRef = useRef<HTMLDivElement | null>(null)
  const areaRef = useRef<SVGPathElement | null>(null)
  const lineRef = useRef<SVGPathElement | null>(null)
  const chartRef = useRef<Chart | null>(null)

  // Declared before the data effect so the chart exists when that runs.
  useEffect(() => {
    const chart: Chart = {
      slide: slideRef.current!,
      stretch: stretchRef.current!,
      area: areaRef.current!,
      line: lineRef.current!,
      size: null,
      data: null,
      buf: null,
      leftGuard: null,
      scale: null,
      slideAnim: null,
      stretchAnim: null,
    }
    chartRef.current = chart

    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect
      if (chart.size?.width === width && chart.size.height === height) {
        return
      }

      chart.size = width && height ? { width, height } : null
      drawSettled(chart)
    })
    observer.observe(rootRef.current!)

    return () => {
      observer.disconnect()
      stopAnimations(chart)
      chartRef.current = null
    }
  }, [])

  useEffect(() => {
    const chart = chartRef.current!
    const prevData = chart.data
    // Widgets re-render on every clash ws event and pass a freshly built array,
    // so only a change in content means a new sample arrived. Leave the running
    // animation untouched otherwise.
    if (prevData && isEqual(prevData, data)) {
      return
    }
    chart.data = data.slice()

    const shift =
      prevData && chart.buf && data.length >= 2 ? findShift(prevData, data) : 0

    if (shift) {
      scrollIn(chart, data, shift, animationDuration * 1000)
    } else {
      drawSettled(chart)
    }
  }, [data, animationDuration])

  return (
    <div
      ref={rootRef}
      data-slot="sparkline"
      className={cn('size-full overflow-hidden', className)}
      {...props}
    >
      <div ref={slideRef} className="h-full will-change-transform">
        <div
          ref={stretchRef}
          className="size-full origin-bottom will-change-transform"
        >
          <svg className="size-full overflow-visible">
            <path ref={areaRef} className="area fill-primary/10" />
            <path
              ref={lineRef}
              className="line stroke-primary"
              fill="none"
              strokeWidth={2}
            />
          </svg>
        </div>
      </div>
    </div>
  )
}
