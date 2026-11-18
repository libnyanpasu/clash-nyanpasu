import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { test } from 'node:test'
import {
  cliSidecarTargetFile,
  planCliCargoBuild,
  prepareCliSidecarAndCompletions,
} from './cli.ts'

test('cliSidecarTargetFile appends target triple and Windows extension', () => {
  assert.strictEqual(
    cliSidecarTargetFile('x86_64-unknown-linux-gnu'),
    'nyanpasu-cli-x86_64-unknown-linux-gnu',
  )
  assert.strictEqual(
    cliSidecarTargetFile('aarch64-apple-darwin'),
    'nyanpasu-cli-aarch64-apple-darwin',
  )
  assert.strictEqual(
    cliSidecarTargetFile('x86_64-pc-windows-msvc'),
    'nyanpasu-cli-x86_64-pc-windows-msvc.exe',
  )
})

test('planCliCargoBuild selects cargo for native/Darwin/Windows targets and cross for Linux aarch64 cross-builds', () => {
  const nativePlan = planCliCargoBuild({
    workspaceRoot: '/repo',
    sidecarHost: 'x86_64-unknown-linux-gnu',
    rustHost: 'x86_64-unknown-linux-gnu',
    profile: 'release',
    platform: 'linux',
  })
  assert.strictEqual(nativePlan.command, 'cargo')
  assert.strictEqual(nativePlan.isCross, false)
  assert.strictEqual(
    nativePlan.targetBinaryPath,
    path.join('/repo', 'backend/target/release/nyanpasu-cli'),
  )

  const linuxCrossPlan = planCliCargoBuild({
    workspaceRoot: '/repo',
    sidecarHost: 'aarch64-unknown-linux-gnu',
    rustHost: 'x86_64-unknown-linux-gnu',
    profile: 'release',
    platform: 'linux',
  })
  assert.strictEqual(linuxCrossPlan.command, 'cross')
  assert.strictEqual(linuxCrossPlan.cwd, path.join('/repo', 'backend'))
  assert.strictEqual(linuxCrossPlan.isCross, true)
  assert.deepStrictEqual(linuxCrossPlan.args, [
    'build',
    '--locked',
    '--manifest-path',
    'Cargo.toml',
    '-p',
    'nyanpasu-cli',
    '--bin',
    'nyanpasu-cli',
    '--release',
    '--target',
    'aarch64-unknown-linux-gnu',
  ])

  const winArm64Plan = planCliCargoBuild({
    workspaceRoot: '/repo',
    sidecarHost: 'aarch64-pc-windows-msvc',
    rustHost: 'x86_64-pc-windows-msvc',
    profile: 'release',
    platform: 'win32',
  })
  assert.strictEqual(winArm64Plan.command, 'cargo')
  assert.strictEqual(winArm64Plan.isCross, true)
  assert.strictEqual(
    winArm64Plan.targetBinaryPath,
    path.join(
      '/repo',
      'backend/target/aarch64-pc-windows-msvc/release/nyanpasu-cli.exe',
    ),
  )
})

test('prepareCliSidecarAndCompletions stages native sidecar and generates completions', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'nyanpasu-prepare-cli-'))
  const builtBinary = path.join(root, 'backend/target/release/nyanpasu-cli')
  const sidecarDir = path.join(root, 'backend/tauri/sidecar')
  const completionsDir = path.join(root, 'backend/tauri/completions')
  const calls: { command: string; args: string[] }[] = []

  try {
    await mkdir(path.dirname(builtBinary), { recursive: true })
    await writeFile(builtBinary, 'fake-cli-binary', { mode: 0o755 })

    const info = await prepareCliSidecarAndCompletions({
      workspaceRoot: root,
      sidecarDir,
      completionsDir,
      sidecarHost: 'x86_64-unknown-linux-gnu',
      rustHost: 'x86_64-unknown-linux-gnu',
      profile: 'release',
      platform: 'linux',
      run: async (command, options) => {
        const args = [...(options?.args ?? [])]
        calls.push({ command, args })
        return {
          success: true,
          code: 0,
          stdout: Buffer.from(`comp:${args[1] ?? 'build'}`),
          stderr: Buffer.alloc(0),
        }
      },
    })

    assert.strictEqual(info.file, 'nyanpasu-cli-x86_64-unknown-linux-gnu')
    assert.strictEqual(
      await readFile(path.join(sidecarDir, info.file), 'utf8'),
      'fake-cli-binary',
    )
    assert.strictEqual(calls[0].command, 'cargo')
    assert.strictEqual(
      calls[1].command,
      path.join(sidecarDir, 'nyanpasu-cli-x86_64-unknown-linux-gnu'),
    )
    assert.strictEqual(
      await readFile(path.join(completionsDir, 'nyanpasu-cli.bash'), 'utf8'),
      'comp:bash',
    )
    assert.strictEqual(
      await readFile(path.join(completionsDir, '_nyanpasu-cli.ps1'), 'utf8'),
      'comp:powershell',
    )
  } finally {
    await rm(root, { recursive: true })
  }
})
