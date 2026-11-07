import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

// Both discovery and execution must include RPC independently of desktop features.
test('musl builds and requires the neutral RPC HTTP and local endpoint test target', () => {
  const runner = readFileSync(
    new URL('./test-backend-musl.ts', import.meta.url),
    'utf8',
  )
  assert.match(runner, /-p nyanpasu-rpc /)
  assert.match(runner, /--features nyanpasu-rpc\/http /)
  assert.match(runner, /--features nyanpasu-rpc\/local-server /)
  assert.match(runner, /--features nyanpasu-rpc-protocol\/frame-io /)
  assert.match(runner, /--features nyanpasu-rpc-protocol\/local-client /)
  assert.match(runner, /'lib:nyanpasu_rpc_protocol'/)
  assert.match(runner, /'lib:nyanpasu_rpc',/)
  assert.match(
    runner,
    /'"\$toolchain_cargo" build --locked --manifest-path backend\/Cargo.toml ' \+\s*'-p nyanpasu-rpc-protocol --lib --no-default-features --features local-client --jobs 12 >&2\\n'/,
  )
  assert.match(
    runner,
    /build --locked --manifest-path backend\/Cargo.toml -p nyanpasu-server --lib --bin nyanpasu-server --jobs 12/,
  )
  assert.match(
    runner,
    /-p nyanpasu-server --features nyanpasu-os\/test-support --lib --tests/,
  )
  assert.match(runner, /'lib:nyanpasu_server'/)
  // Cargo --tests includes unit tests for binary targets. Every emitted test
  // artifact must be registered; the runner continues to reject extras.
  assert.match(runner, /'bin:nyanpasu-server'/)
  assert.match(runner, /'test:panic_process'/)
  const production = runner.match(
    /build --locked[^\n]*-p nyanpasu-server[^\n]*/,
  )?.[0]
  assert.ok(production)
  assert.doesNotMatch(production, /panic-injection|--all-features/)
  assert.match(runner, /artifact\.profile\?\.test/)
  assert.match(runner, /unexpected test target/)
})
