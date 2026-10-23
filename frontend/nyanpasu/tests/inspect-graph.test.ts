import { describe, expect, it } from 'vitest'
import {
  edgePath,
  layoutGraph,
  NODE_HEIGHT,
  traceOf,
} from '@/pages/(main)/main/profiles/inspect/_modules/inspect-graph'
import { layoutLanes } from '@/pages/(main)/main/profiles/inspect/_modules/inspect-lanes'
import type {
  Stage,
  Step,
} from '@/pages/(main)/main/profiles/inspect/_modules/inspect-steps'

const step = (id: number, stage: Stage, next: number[] = []): Step =>
  ({
    key: `s${id}`,
    node: { id, next },
    stage,
    depth: 0,
    status: 'unchanged',
    changedCount: 0,
  }) as unknown as Step

describe('layoutGraph', () => {
  it('stacks a chain in one column and opens a band per stage', () => {
    const steps = [
      step(0, 'source', [1]),
      step(1, 'source', [2]),
      step(2, 'result'),
    ]
    const geometry = layoutGraph(steps, layoutLanes(steps, steps))

    expect(new Set(geometry.nodes.map((node) => node.x)).size).toBe(1)
    expect(geometry.bands.map((band) => band.stage)).toEqual([
      'source',
      'result',
    ])
    // Every node sits inside its band.
    geometry.bands.forEach((band, index) => {
      const own = geometry.nodes.filter((_, i) => steps[i].stage === band.stage)
      for (const node of own) {
        expect(node.y).toBeGreaterThan(band.y)
        expect(node.y + NODE_HEIGHT).toBeLessThan(band.y + band.height)
      }
      expect(index === 0 || band.y > geometry.bands[index - 1].y).toBe(true)
    })
    expect(geometry.edges.map((edge) => edge.d)[0]).toMatch(/^M \d+ \d+ V /)
  })

  it('puts a fork in a second column', () => {
    const steps = [
      step(0, 'source', [1, 2]),
      step(1, 'source', [3]),
      step(2, 'source', [3]),
      step(3, 'result'),
    ]
    const geometry = layoutGraph(steps, layoutLanes(steps, steps))

    expect(geometry.nodes[2].x).toBeGreaterThan(geometry.nodes[1].x)
    expect(geometry.nodes[3].x).toBe(geometry.nodes[0].x)
    expect(geometry.width).toBeGreaterThan(geometry.nodes[2].x)
  })
})

describe('edgePath', () => {
  it('is a straight line inside one column', () => {
    expect(edgePath({ x: 10, y: 0 }, { x: 10, y: 80 }, 10)).toBe(
      'M 10 0 V 60 V 80',
    )
  })

  it('turns towards another column and back', () => {
    const d = edgePath({ x: 10, y: 0 }, { x: 10, y: 200 }, 266)
    expect(d).toContain('H 258')
    expect(d).toContain('H 18')
    expect(d.endsWith('V 200')).toBe(true)
  })
})

describe('traceOf', () => {
  it('follows the chain both ways but not into sibling branches', () => {
    const edges = [
      { from: 'a', to: 'b' },
      { from: 'a', to: 'c' },
      { from: 'b', to: 'd' },
      { from: 'c', to: 'd' },
      { from: 'd', to: 'e' },
    ]
    expect([...traceOf(edges, 'b')].sort()).toEqual(['a', 'b', 'd', 'e'])
    expect([...traceOf(edges, 'e')].sort()).toEqual(['a', 'b', 'c', 'd', 'e'])
  })
})
