import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

// Both discovery and execution must include RPC independently of desktop features.
test('musl builds and requires the neutral RPC HTTP test target', () => {
  const runner = readFileSync(
    new URL('./test-backend-musl.ts', import.meta.url),
    'utf8',
  )
  assert.match(runner, /-p nyanpasu-rpc /)
  assert.match(runner, /--features nyanpasu-rpc\/http /)
  assert.match(runner, /'lib:nyanpasu_rpc',/)
})
