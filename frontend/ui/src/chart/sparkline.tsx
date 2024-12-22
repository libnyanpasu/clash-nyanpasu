import * as d3 from 'd3'
import { interpolatePath } from 'd3-interpolate-path'
import { CSSProperties, FC, useEffect, useRef } from 'react'
import { alpha, useTheme } from '@mui/material'

interface SparklineProps {
  data: number[]
  className?: string
  style?: CSSProperties
}

export const Sparkline: FC<SparklineProps> = ({ data, className, style }) => {
  const { palette } = useTheme()

  const svgRef = useRef<SVGSVGElement | null>(null)

  useEffect(() => {
    if (!svgRef.current) return

    const svg = d3.select(svgRef.current)

    const { width, height } = svg.node()?.getBoundingClientRect() ?? {
      width: 0,
      height: 0,
    }

    const maxHeight = () => {
      const dataRange = d3.max(data)! - d3.min(data)!

      if (dataRange / d3.max(data)! < 0.1) {
        return height * 0.65
      }

      if (d3.max(data)) {
        return height * 0.35
      } else {
        return height
      }
    }

    const xScale = d3
      .scaleLinear()
      .domain([0, data.length - 1])
      .range([0, width])

    const yScale = d3
      .scaleLinear()
      .domain([0, d3.max(data) ?? 0])
      .range([height, maxHeight()])

    const line = d3
      .line<number>()
      .x((d, i) => xScale(i))
      .y((d) => yScale(d))
      .curve(d3.curveCatmullRom.alpha(0.5))

    const area = d3
      .area<number>()
      .x((d, i) => xScale(i))
      .y0(height)
      .y1((d) => yScale(d))
      .curve(d3.curveCatmullRom.alpha(0.5))

    svg.selectAll('*').remove()

    svg
      .append('path')
      .datum(data)
      .attr('class', 'area')
      .attr('fill', alpha(palette.primary.main, 0.1))
      .attr('d', area)

    svg
      .append('path')
      .datum(data)
      .attr('class', 'line')
      .attr('fill', 'none')
      .attr('stroke', alpha(palette.primary.main, 0.7))
      .attr('stroke-width', 2)
      .attr('d', line)

    const updateChart = () => {
      xScale.domain([0, data.length - 1])
      yScale.domain([0, d3.max(data) ?? 0])

      const t = svg.transition().duration(750).ease(d3.easeCubic)
      svg
        .select('.area')
        .datum(data)
        .transition(t as any)
        .attrTween('d', function (d) {
          const previous = d3.select(this).attr('d')
          const current = area(d)
          return interpolatePath(previous, current as string)
        })

      svg
        .select('.line')
        .datum(data)
        .transition(t as any)
        .attrTween('d', function (d) {
          const previous = d3.select(this).attr('d')
          const current = line(d)
          return interpolatePath(previous, current as string)
        })
    }

    updateChart()
  }, [data, palette.primary.main])

  return (
    <svg
      className={className}
      ref={svgRef}
      style={{ width: '100%', height: '100%', ...style }}
    />
  )
}
