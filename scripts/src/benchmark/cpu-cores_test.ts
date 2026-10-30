import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  affinityMask,
  chooseProcessors,
  parseCpuList,
  type Processor,
} from './cpu-cores.ts'

// Two performance cores with two threads each, then two efficiency cores, as
// a hybrid Intel part reports them.
const HYBRID: Processor[] = [
  { processor: 0, core: 0, efficiency: 1 },
  { processor: 1, core: 0, efficiency: 1 },
  { processor: 2, core: 2, efficiency: 1 },
  { processor: 3, core: 2, efficiency: 1 },
  { processor: 4, core: 4, efficiency: 0 },
  { processor: 5, core: 5, efficiency: 0 },
]

test('chooseProcessors takes one thread per core of the kind', () => {
  assert.deepStrictEqual(chooseProcessors(HYBRID, 'performance'), [0, 2])
  assert.deepStrictEqual(chooseProcessors(HYBRID, 'efficiency'), [4, 5])
})

test('chooseProcessors takes every core of a single class', () => {
  const uniform = HYBRID.map((processor) => ({ ...processor, efficiency: 0 }))

  assert.deepStrictEqual(chooseProcessors(uniform, 'performance'), [0, 2, 4, 5])
  assert.deepStrictEqual(chooseProcessors(uniform, 'efficiency'), [0, 2, 4, 5])
})

test('affinityMask sets a bit per processor, past 32 as well', () => {
  assert.deepStrictEqual(affinityMask([0, 2, 4, 6]), '55')
  assert.deepStrictEqual(affinityMask([16, 31]), '80010000')
  assert.deepStrictEqual(affinityMask([40]), '10000000000')
})

test('parseCpuList expands ranges and single processors', () => {
  assert.deepStrictEqual(parseCpuList('0-3,8,10-11\n'), [0, 1, 2, 3, 8, 10, 11])
  assert.deepStrictEqual(parseCpuList(''), [])
})
