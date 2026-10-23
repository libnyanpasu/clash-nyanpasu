import assert from 'node:assert/strict'
import { test } from 'node:test'
import { runPublicationTasks } from './publication-concurrency.ts'

test('publication tasks bound concurrency, drain failures and preserve input order', async () => {
  const release = Promise.withResolvers<void>()
  const started = Promise.withResolvers<void>()
  let active = 0
  let maximum = 0
  const seen: number[] = []
  const result = runPublicationTasks([0, 1, 2, 3, 4], 2, async (value) => {
    active++
    maximum = Math.max(maximum, active)
    seen.push(value)
    if (seen.length === 2) started.resolve()
    await release.promise
    active--
    if (value === 1) throw new Error('target failed')
    return value * 2
  })
  await started.promise
  assert.deepStrictEqual(seen, [0, 1])
  release.resolve()
  const results = await result
  assert.deepStrictEqual(maximum, 2)
  assert.deepStrictEqual(seen, [0, 1, 2, 3, 4])
  assert.deepStrictEqual(
    results.map((entry) => entry.status),
    ['fulfilled', 'rejected', 'fulfilled', 'fulfilled', 'fulfilled'],
  )
  assert.deepStrictEqual(results[4], { status: 'fulfilled', value: 8 })
  assert.deepStrictEqual(await runPublicationTasks([], 2, async () => 0), [])
  await assert.rejects(() => runPublicationTasks([0], 0, async () => 0))
})
