import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { test } from 'node:test'
import { isNotFound } from './errors.ts'

test('isNotFound recognizes a missing path only', async () => {
  const missing = await readFile('/nonexistent/nyanpasu-scripts').catch(
    (e) => e,
  )
  assert.deepStrictEqual(isNotFound(missing), true)
  assert.deepStrictEqual(isNotFound(new Error('other')), false)
  assert.deepStrictEqual(isNotFound('ENOENT'), false)
})
