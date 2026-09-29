import * as d3 from 'd3'
import { isEqual } from 'lodash-es'
import { animate } from 'motion/react'
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

export const Sparkline = ({
  data,
  animationDuration = 1,
  className,
  ...props
}: ComponentPropsWithoutRef<'svg'> & {
  data: number[]
  animationDuration?: number
}) => {
  const svgRef = useRef<SVGSVGElement | null>(null)
  const gRef = useRef<SVGGElement | null>(null)
  const prevDataRef = useRef<number[] | null>(null)
  // Every point currently drawn; its last N points always equal the latest
  // series, and the ones before them are still scrolling off to the left.
  const bufRef = useRef<number[] | null>(null)
  // Current scroll position of the buffer, in steps.
  const offsetRef = useRef(0)
  // The point just before bufRef[0], so the curve shape at the left edge stays
  // the same when scrolled-off points are dropped from the buffer.
  const leftGuardRef = useRef<number | null>(null)
  const animRef = useRef<ReturnType<typeof animate> | null>(null)

  useEffect(() => {
    if (!svgRef.current || !gRef.current) {
      return
    }

    const g = d3.select(gRef.current)
    const { width, height } = svgRef.current.getBoundingClientRect()
    if (!width || !height) {
      return
    }

    const makePaths = (
      points: number[],
      xRange: [number, number],
      yMax: number,
    ) => {
      const mean = d3.mean(points) ?? 0
      const std = d3.deviation(points) ?? 0
      const cv = mean > 0 ? std / mean : 0
      const topFactor =
        yMax === 0
          ? 1
          : cv < STABLE_CV_THRESHOLD
            ? STABLE_TOP_FACTOR
            : ACTIVE_TOP_FACTOR

      const x = d3
        .scaleLinear()
        .domain([0, points.length - 1])
        .range(xRange)
      const y = d3
        .scaleLinear()
        .domain([0, yMax])
        .range([height, height * topFactor])

      const lineGen = d3
        .line<number>()
        .x((_, i) => x(i))
        .y((d) => y(d))
        .curve(d3.curveCatmullRom.alpha(0.5))
      const areaGen = d3
        .area<number>()
        .x((_, i) => x(i))
        .y0(height)
        .y1((d) => y(d))
        .curve(d3.curveCatmullRom.alpha(0.5))

      return {
        line: lineGen(points) ?? '',
        area: areaGen(points) ?? '',
      }
    }

    // Prepend/append invisible guard points one step outside each edge so that
    // every visible data point is treated as an interior spline node, eliminating
    // the endpoint tangent discontinuity that causes boundary wobble.
    // SVG overflow:hidden clips the guard region automatically.
    const buildPaths = (
      points: number[],
      xRange: [number, number],
      yMax: number,
      step: number,
      leftGuard?: number,
    ) => {
      const n = points.length

      // Fast-path for empty or single-point series to avoid invalid guard math.
      if (n === 0) {
        // No data → no path.
        return { line: '', area: '' }
      }

      if (n === 1) {
        // Single point: render a degenerate path without guard extension.
        return makePaths(points, xRange, yMax)
      }

      const lGuard = leftGuard ?? 2 * points[0] - points[1]
      const rGuard = 2 * points[n - 1] - points[n - 2]

      return makePaths(
        [lGuard, ...points, rGuard],
        [xRange[0] - step, xRange[1] + step],
        yMax,
      )
    }

    const prevData = prevDataRef.current
    // Widgets re-render on every clash ws event and pass a freshly built array,
    // so only a change in content means a new sample arrived. Leave the running
    // animation untouched otherwise.
    if (prevData && isEqual(prevData, data)) {
      return
    }
    prevDataRef.current = data.slice()

    const running = animRef.current
    animRef.current = null
    running?.stop()

    // Handle short series early to avoid division by zero and invalid indexing.
    if (data.length < 2) {
      g.selectAll('*').remove()
      g.attr('transform', 'translate(0,0)')
      bufRef.current = null
      offsetRef.current = 0
      leftGuardRef.current = null
      return
    }

    const stepWidth = width / (data.length - 1)
    const prevBuf = bufRef.current
    const shift = prevData && prevBuf ? findShift(prevData, data) : 0

    if (!shift || !prevBuf) {
      const yMax = Math.max(d3.max(data) ?? 0, 1)
      const { line, area } = buildPaths(
        data,
        [0, width],
        yMax,
        stepWidth,
        leftGuardRef.current ?? undefined,
      )

      bufRef.current = data.slice()
      offsetRef.current = 0

      g.selectAll('*').remove()
      g.attr('transform', 'translate(0,0)')
      g.append('path').attr('class', 'area fill-primary/10').attr('d', area)
      g.append('path')
        .attr('class', 'line stroke-primary')
        .attr('fill', 'none')
        .attr('stroke-width', 2)
        .attr('d', line)
      return
    }

    // A new sample may arrive before the previous scroll finished. Continue from
    // the current position instead of snapping to a cycle boundary: drop the
    // points that have fully scrolled off, append the new samples and scroll on.
    const scrolled = Math.floor(offsetRef.current)
    let buf = prevBuf
    if (scrolled > 0) {
      leftGuardRef.current = buf[scrolled - 1]
      buf = buf.slice(scrolled)
    }
    buf = [...buf, ...data.slice(-shift)]
    bufRef.current = buf

    const leftGuard = leftGuardRef.current ?? undefined
    const fromOffset = offsetRef.current - scrolled
    const toOffset = buf.length - data.length
    const bufRange: [number, number] = [0, stepWidth * (buf.length - 1)]

    const fromYMax = Math.max(d3.max(buf) ?? 0, 1)
    const toYMax = Math.max(d3.max(data) ?? 0, 1)
    const yMaxChanges = Math.abs(fromYMax - toYMax) > 1

    const setOffset = (offset: number) => {
      offsetRef.current = offset
      g.attr('transform', `translate(${-stepWidth * offset},0)`)
    }

    // Render the initial (pre-animation) state.
    const { line: initLine, area: initArea } = buildPaths(
      buf,
      bufRange,
      fromYMax,
      stepWidth,
      leftGuard,
    )

    setOffset(fromOffset)
    g.select('.area').attr('d', initArea)
    g.select('.line').attr('d', initLine)

    const anim = animate(0, 1, {
      duration: animationDuration,
      ease: 'linear',
      onUpdate(t) {
        // X-axis: pure linear translation — the scroll must feel constant-speed.
        setOffset(fromOffset + (toOffset - fromOffset) * t)

        // Y-axis: non-linear easing for the yMax interpolation so the height
        // change feels more natural (slow start/end, faster in the middle).
        // Because x is driven by the translation and y is driven independently
        // by yMax, the two axes never couple — no wobble.
        if (yMaxChanges) {
          const easedT = d3.easeCubicInOut(t)
          const currentYMax = fromYMax + (toYMax - fromYMax) * easedT
          const { line, area } = buildPaths(
            buf,
            bufRange,
            currentYMax,
            stepWidth,
            leftGuard,
          )

          g.select('.area').attr('d', area)
          g.select('.line').attr('d', line)
        }
      },
      onComplete() {
        if (animRef.current !== anim) {
          return
        }

        // The last scrolled-off point becomes the left guard for the next cycle
        // so the curve shape at x=0 stays consistent across animation boundaries.
        leftGuardRef.current = buf[toOffset - 1]
        bufRef.current = data.slice()

        // At the end of the scroll the buffer path at -toOffset steps and the
        // N-point path at x=0 occupy identical visual coordinates, so the swap
        // is seamless.
        const { line, area } = buildPaths(
          data,
          [0, width],
          toYMax,
          stepWidth,
          leftGuardRef.current,
        )

        setOffset(0)
        g.select('.area').attr('d', area)
        g.select('.line').attr('d', line)
      },
    })

    animRef.current = anim
  }, [data, animationDuration])

  useEffect(() => () => animRef.current?.stop(), [])

  return (
    <svg
      ref={svgRef}
      data-slot="sparkline"
      className={cn('size-full overflow-hidden', className)}
      {...props}
    >
      <g ref={gRef} />
    </svg>
  )
}
