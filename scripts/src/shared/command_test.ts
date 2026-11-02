import assert from 'node:assert/strict'
import { test } from 'node:test'
import { exitStatus, runCommand, spawnCommand } from './command.ts'

test('runCommand captures output and a non-zero exit without throwing', async () => {
  const result = await runCommand('sh', {
    args: ['-c', 'printf out; printf err >&2; exit 3'],
  })
  assert.deepStrictEqual(result.success, false)
  assert.deepStrictEqual(result.code, 3)
  assert.deepStrictEqual(new TextDecoder().decode(result.stdout), 'out')
  assert.deepStrictEqual(new TextDecoder().decode(result.stderr), 'err')
})

test('runCommand merges env into the parent environment unless cleared', async () => {
  const print = [
    '-e',
    'process.stdout.write(JSON.stringify([process.env.NYANPASU_A, process.env.NYANPASU_INHERITED]))',
  ]
  const previous = process.env.NYANPASU_INHERITED
  process.env.NYANPASU_INHERITED = 'parent'
  try {
    const merged = await runCommand(process.execPath, {
      args: print,
      env: { NYANPASU_A: 'a' },
    })
    assert.deepStrictEqual(JSON.parse(merged.stdout.toString()), [
      'a',
      'parent',
    ])
    const cleared = await runCommand(process.execPath, {
      args: print,
      env: { NYANPASU_A: 'a' },
      clearEnv: true,
    })
    assert.deepStrictEqual(JSON.parse(cleared.stdout.toString()), ['a', null])
  } finally {
    if (previous === undefined) delete process.env.NYANPASU_INHERITED
    else process.env.NYANPASU_INHERITED = previous
  }
})

test('runCommand rejects when the program does not exist', async () => {
  await assert.rejects(() => runCommand('nyanpasu-missing-program'))
})

test('spawnCommand feeds piped stdin and reports the exit status', async () => {
  const child = spawnCommand('sh', {
    args: ['-c', 'read line; test "$line" = ping'],
    stdin: 'piped',
    stderr: 'null',
  })
  child.stdin!.end('ping\n')
  assert.deepStrictEqual((await exitStatus(child)).success, true)
})

test('runCommand writes input to stdin and captures the output', async () => {
  const result = await runCommand('cat', { input: 'ping\n' })
  assert.deepStrictEqual(result.success, true)
  assert.deepStrictEqual(result.stdout.toString('utf8'), 'ping\n')
})

test(
  'exitStatus maps a signal exit to 128 plus the signal number',
  { skip: process.platform === 'win32' },
  async () => {
    const child = spawnCommand(process.execPath, {
      args: ['-e', "process.kill(process.pid, 'SIGTERM')"],
    })
    assert.deepStrictEqual(await exitStatus(child), {
      success: false,
      code: 143,
    })
  },
)
