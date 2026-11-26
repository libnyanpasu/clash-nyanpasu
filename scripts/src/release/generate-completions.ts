import { mkdir, writeFile } from 'node:fs/promises'
import * as path from 'node:path'
import { parseArgs } from 'node:util'
import { runCommand, type CommandOptions } from '../shared/command.ts'
import { consola } from '../shared/logger.ts'
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts'

export interface CompletionTarget {
  shell: 'bash' | 'zsh' | 'fish' | 'powershell'
  fileName: string
}

export const COMPLETION_TARGETS: readonly CompletionTarget[] = [
  { shell: 'bash', fileName: 'nyanpasu-cli.bash' },
  { shell: 'zsh', fileName: '_nyanpasu-cli' },
  { shell: 'fish', fileName: 'nyanpasu-cli.fish' },
  { shell: 'powershell', fileName: '_nyanpasu-cli.ps1' },
] as const

export interface GenerateCompletionsOptions {
  outputDir: string
  workspaceRoot?: string
  executablePath?: string
  profile?: 'release' | 'dev'
  run?: (
    command: string,
    options?: CommandOptions,
  ) => Promise<{
    success: boolean
    code: number
    stdout: Buffer
    stderr: Buffer
  }>
}

export function resolveHostCompletionBinary(
  workspaceRoot: string,
  profile: 'release' | 'dev',
  platform: NodeJS.Platform = process.platform,
): string {
  const ext = platform === 'win32' ? '.exe' : ''
  const profileDir = profile === 'release' ? 'release-cli' : 'debug'
  return path.join(
    workspaceRoot,
    'backend/target',
    profileDir,
    `nyanpasu-cli${ext}`,
  )
}

export async function generateCompletions(
  options: GenerateCompletionsOptions,
): Promise<string[]> {
  const workspaceRoot = options.workspaceRoot ?? WORKSPACE_ROOT
  const profile = options.profile ?? 'release'
  const exec = options.run ?? runCommand
  let binaryPath = options.executablePath

  if (!binaryPath) {
    const buildArgs = [
      'build',
      '--locked',
      '--manifest-path',
      'backend/Cargo.toml',
      '-p',
      'nyanpasu-cli',
      '--bin',
      'nyanpasu-cli',
      ...(profile === 'release' ? ['--profile', 'release-cli'] : []),
    ]
    const built = await exec('cargo', {
      cwd: workspaceRoot,
      args: buildArgs,
      stdout: 'piped',
      stderr: 'piped',
    })
    if (!built.success) {
      throw new Error(
        `failed to build host nyanpasu-cli for completions:\n${built.stderr.toString('utf8')}`,
      )
    }
    binaryPath = resolveHostCompletionBinary(workspaceRoot, profile)
  }

  await mkdir(options.outputDir, { recursive: true })
  const written: string[] = []
  for (const target of COMPLETION_TARGETS) {
    const result = await exec(binaryPath, {
      cwd: workspaceRoot,
      args: ['completion', target.shell],
      stdout: 'piped',
      stderr: 'piped',
    })
    if (!result.success) {
      throw new Error(
        `failed to generate ${target.shell} completions:\n${result.stderr.toString('utf8')}`,
      )
    }
    const destination = path.join(options.outputDir, target.fileName)
    await writeFile(destination, result.stdout)
    written.push(destination)
  }
  return written
}

async function main(): Promise<void> {
  const { values } = parseArgs({
    options: {
      'output-dir': { type: 'string' },
      executable: { type: 'string' },
      profile: { type: 'string' },
    },
    strict: true,
  })
  const profile = values.profile === 'dev' ? 'dev' : 'release'
  const outputDir = path.resolve(
    values['output-dir'] ??
      path.join(WORKSPACE_ROOT, 'backend/tauri/completions'),
  )
  const files = await generateCompletions({
    outputDir,
    executablePath: values.executable
      ? path.resolve(values.executable)
      : undefined,
    profile,
  })
  consola.success(
    `generated ${files.length} completion scripts in ${outputDir}`,
  )
}

if (import.meta.main) {
  main().catch((err) => {
    consola.error(err)
    process.exit(1)
  })
}
