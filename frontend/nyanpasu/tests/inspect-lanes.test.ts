import { describe, expect, it } from 'vitest'
import {
  layoutLanes,
  stepEdges,
} from '@/pages/(main)/main/profiles/inspect/_modules/inspect-lanes'
import type { Step } from '@/pages/(main)/main/profiles/inspect/_modules/inspect-steps'

// Only the key and the node's id and edges matter to the layout.
const step = (id: number, next: number[] = []): Step =>
  ({
    key: `s${id}`,
    node: { id, next },
    stage: 'source',
    depth: 0,
    status: 'unchanged',
    changedCount: 0,
  }) as unknown as Step

const lanesOf = (steps: Step[], visible = steps) =>
  layoutLanes(steps, visible).rows.map(({ key, lane }) => [key, lane])

describe('layoutLanes', () => {
  it('keeps a chain in one lane', () => {
    const steps = [step(0, [1]), step(1, [2]), step(2)]
    const layout = layoutLanes(steps, steps)

    expect(layout.laneCount).toBe(1)
    expect(layout.rows.map((row) => row.shape)).toEqual([
      'start',
      'step',
      'step',
    ])
    expect(layout.rows.map((row) => [row.incoming, row.outgoing])).toEqual([
      [false, true],
      [true, true],
      [true, false],
    ])
  })

  it('opens a lane for a fork and closes it at the merge', () => {
    // 0 -> 1 -> 3 and 0 -> 2 -> 3
    const steps = [step(0, [1, 2]), step(1, [3]), step(2, [3]), step(3)]
    const layout = layoutLanes(steps, steps)

    expect(lanesOf(steps)).toEqual([
      ['s0', 0],
      ['s1', 0],
      ['s2', 1],
      ['s3', 0],
    ])
    expect(layout.rows[0]).toMatchObject({ shape: 'start', forks: [1] })
    expect(layout.rows[1].after).toEqual([0, 1])
    expect(layout.rows[2].through).toEqual([0])
    expect(layout.rows[3]).toMatchObject({ shape: 'merge', merges: [1] })
    expect(layout.laneCount).toBe(2)
  })

  it('starts a second root in a free lane', () => {
    const steps = [step(0, [2]), step(1, [2]), step(2)]
    const layout = layoutLanes(steps, steps)

    expect(lanesOf(steps)).toEqual([
      ['s0', 0],
      ['s1', 1],
      ['s2', 0],
    ])
    expect(layout.rows[1].shape).toBe('start')
    expect(layout.rows[2].shape).toBe('merge')
  })

  it('bypasses a hidden step and drops back edges', () => {
    const steps = [step(0, [1]), step(1, [2]), step(2, [0])]

    expect(stepEdges(steps, [steps[0], steps[2]])).toEqual([
      { from: 's0', to: 's2' },
    ])
    expect(stepEdges(steps, steps)).toEqual([
      { from: 's0', to: 's1' },
      { from: 's1', to: 's2' },
    ])
  })
})
