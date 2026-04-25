import * as d3 from 'd3'
import { animate } from 'framer-motion'
import { cloneDeep } from 'lodash-es'
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
  // Tracks the most recently scrolled-off left point so successive cycles share
  // the same left guard value, making the curve at x=0 seamless across transitions.
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
    prevDataRef.current = cloneDeep(data)

    animRef.current?.stop()
    animRef.current = null

    // Handle short series early to avoid division by zero and invalid indexing.
    if (data.length < 2) {
      g.selectAll('*').remove()
      g.attr('transform', 'translate(0,0)')
      leftGuardRef.current = null
      return
    }

    if (!prevData || prevData.length !== data.length) {
      const yMax = Math.max(d3.max(data) ?? 0, 1)
      const step = width / (data.length - 1)
      const { line, area } = buildPaths(
        data,
        [0, width],
        yMax,
        step,
        leftGuardRef.current ?? undefined,
      )

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

    const stepWidth = width / (data.length - 1)

    // N+1 points: the old leading point (about to scroll off) + the full new data array.
    const extPoints = [...prevData, data[data.length - 1]]
    const fromYMax = Math.max(d3.max(extPoints) ?? 0, 1)
    const toYMax = Math.max(d3.max(data) ?? 0, 1)
    const yMaxChanges = Math.abs(fromYMax - toYMax) > 1

    // Use the stored left guard so the CatmullRom context at x=0 is identical
    // between the N-point path rendered before this animation and the N+1-point
    // path at t=0, making the left edge transition seamless.
    const leftGuard = leftGuardRef.current ?? undefined

    // Render the initial (pre-animation) state.
    const { line: initLine, area: initArea } = buildPaths(
      extPoints,
      [0, width + stepWidth],
      fromYMax,
      stepWidth,
      leftGuard,
    )

    g.attr('transform', 'translate(0,0)')
    g.select('.area').attr('d', initArea)
    g.select('.line').attr('d', initLine)

    let cancelled = false

    const anim = animate(0, 1, {
      duration: animationDuration,
      ease: 'linear',
      onUpdate(t) {
        // X-axis: pure linear translation — the scroll must feel constant-speed.
        g.attr('transform', `translate(${-stepWidth * t},0)`)

        // Y-axis: non-linear easing for the yMax interpolation so the height
        // change feels more natural (slow start/end, faster in the middle).
        // Because x is driven by the translation and y is driven independently
        // by yMax, the two axes never couple — no wobble.
        if (yMaxChanges) {
          const easedT = d3.easeCubicInOut(t)
          const currentYMax = fromYMax + (toYMax - fromYMax) * easedT
          const { line, area } = buildPaths(
            extPoints,
            [0, width + stepWidth],
            currentYMax,
            stepWidth,
            leftGuard,
          )

          g.select('.area').attr('d', area)
          g.select('.line').attr('d', line)
        }
      },
      onComplete() {
        if (cancelled) {
          return
        }

        // The scrolled-off point becomes the left guard for the next cycle so
        // the curve shape at x=0 stays consistent across animation boundaries.
        leftGuardRef.current = prevData[0]

        // At t=1 the N+1-point path at -stepWidth and the N-point path at x=0
        // occupy identical visual coordinates, so the swap is seamless.
        const { line, area } = buildPaths(
          data,
          [0, width],
          toYMax,
          stepWidth,
          prevData[0],
        )

        g.attr('transform', 'translate(0,0)')
        g.select('.area').attr('d', area)
        g.select('.line').attr('d', line)
      },
    })

    animRef.current = anim

    return () => {
      cancelled = true
      anim.stop()
    }
  }, [data, animationDuration])

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
