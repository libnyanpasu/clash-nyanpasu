import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { test } from 'node:test'
import {
  COMPLETION_TARGETS,
  generateCompletions,
  resolveHostCompletionBinary,
} from './generate-completions.ts'

test('resolveHostCompletionBinary locates host release and dev binaries', () => {
  assert.deepStrictEqual(
    resolveHostCompletionBinary('/repo', 'release', 'linux'),
    path.join('/repo', 'backend/target/release-cli/nyanpasu-cli'),
  )
  assert.deepStrictEqual(
    resolveHostCompletionBinary('/repo', 'dev', 'win32'),
    path.join('/repo', 'backend/target/debug/nyanpasu-cli.exe'),
  )
})

test('generateCompletions writes all supported shells using the provided executable', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'nyanpasu-completions-'))
  const invocations: string[][] = []
  try {
    const written = await generateCompletions({
      outputDir: root,
      executablePath: '/mock/nyanpasu-cli',
      run: async (_command, options) => {
        const args = [...(options?.args ?? [])]
        invocations.push(args)
        return {
          success: true,
          code: 0,
          stdout: Buffer.from(`# completion for ${args[1]}\n`),
          stderr: Buffer.alloc(0),
        }
      },
    })
    assert.deepStrictEqual(
      invocations,
      COMPLETION_TARGETS.map((target) => ['completion', target.shell]),
    )
    assert.strictEqual(written.length, COMPLETION_TARGETS.length)
    for (const target of COMPLETION_TARGETS) {
      assert.strictEqual(
        await readFile(path.join(root, target.fileName), 'utf8'),
        `# completion for ${target.shell}\n`,
      )
    }
  } finally {
    await rm(root, { recursive: true })
  }
})

test('generateCompletions builds the host CLI when no executablePath is supplied', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'nyanpasu-completions-build-'))
  const calls: { command: string; args: string[] }[] = []
  try {
    await generateCompletions({
      outputDir: root,
      workspaceRoot: '/repo',
      profile: 'dev',
      run: async (command, options) => {
        calls.push({ command, args: [...(options?.args ?? [])] })
        return {
          success: true,
          code: 0,
          stdout: Buffer.from('ok'),
          stderr: Buffer.alloc(0),
        }
      },
    })
    assert.strictEqual(calls[0].command, 'cargo')
    assert.deepStrictEqual(calls[0].args, [
      'build',
      '--locked',
      '--manifest-path',
      'backend/Cargo.toml',
      '-p',
      'nyanpasu-cli',
      '--bin',
      'nyanpasu-cli',
    ])
    assert.strictEqual(calls.length, 1 + COMPLETION_TARGETS.length)
  } finally {
    await rm(root, { recursive: true })
  }
})
