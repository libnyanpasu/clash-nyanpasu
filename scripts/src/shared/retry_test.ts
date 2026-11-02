import assert from 'node:assert/strict'
import { test } from 'node:test'
import { retry, RetryError } from './retry.ts'

test('retry returns the first successful result', async () => {
  let calls = 0
  const value = await retry(() => {
    calls++
    return Promise.resolve('done')
  })
  assert.deepStrictEqual([value, calls], ['done', 1])
})

test('retry stops at a non-retriable error and rethrows it', async () => {
  const failure = new Error('fatal')
  let calls = 0
  await assert.rejects(
    () =>
      retry(
        () => {
          calls++
          throw failure
        },
        { isRetriable: () => false },
      ),
    (error) => error === failure,
  )
  assert.deepStrictEqual(calls, 1)
})

test('retry reports the last error after the attempt limit', async () => {
  const failure = new Error('flaky')
  let calls = 0
  await assert.rejects(
    () =>
      retry(
        () => {
          calls++
          throw failure
        },
        { maxAttempts: 1 },
      ),
    (error) =>
      error instanceof RetryError &&
      /Retrying exceeded the maxAttempts \(1\)\./.test(error.message) &&
      error.cause === failure,
  )
  assert.deepStrictEqual(calls, 1)
})
