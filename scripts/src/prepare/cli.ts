import { chmod, copyFile, mkdir, stat } from 'node:fs/promises'
import * as path from 'node:path'
import { generateCompletions } from '../release/generate-completions.ts'
import { runCommand, type CommandOptions } from '../shared/command.ts'
import type { ResolveInfo } from './types.ts'

export type CliBuildProfile = 'release' | 'dev'

export interface CliCargoPlan {
  command: string
  args: string[]
  cwd: string
  targetBinaryPath: string
  isCross: boolean
}

export interface PrepareCliOptions {
  workspaceRoot: string
  sidecarDir: string
  completionsDir: string
  sidecarHost: string
  rustHost: string
  profile?: CliBuildProfile
  prebuiltBinaryPath?: string
  platform?: NodeJS.Platform
  debugLog?: (message: string) => void
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

export function cliSidecarTargetFile(sidecarHost: string): string {
  const ext = sidecarHost.includes('windows') ? '.exe' : ''
  return `nyanpasu-cli-${sidecarHost}${ext}`
}

export function planCliCargoBuild(options: {
  workspaceRoot: string
  sidecarHost: string
  rustHost: string
  profile: CliBuildProfile
  platform?: NodeJS.Platform
}): CliCargoPlan {
  const { workspaceRoot, sidecarHost, rustHost, profile } = options
  const platform = options.platform ?? process.platform
  const isCross = sidecarHost !== rustHost
  const useCrossContainer =
    isCross && platform === 'linux' && sidecarHost.includes('linux')

  const command = useCrossContainer ? 'cross' : 'cargo'
  const cwd = useCrossContainer
    ? path.join(workspaceRoot, 'backend')
    : workspaceRoot
  const manifestPath = useCrossContainer ? 'Cargo.toml' : 'backend/Cargo.toml'
  const args = [
    'build',
    '--locked',
    '--manifest-path',
    manifestPath,
    '-p',
    'nyanpasu-cli',
    '--bin',
    'nyanpasu-cli',
    ...(profile === 'release' ? ['--release'] : []),
    ...(isCross ? ['--target', sidecarHost] : []),
  ]

  const ext = sidecarHost.includes('windows') ? '.exe' : ''
  const profileDir = profile === 'release' ? 'release' : 'debug'
  const targetBinaryPath = path.join(
    workspaceRoot,
    'backend/target',
    ...(isCross ? [sidecarHost] : []),
    profileDir,
    `nyanpasu-cli${ext}`,
  )

  return { command, args, cwd, targetBinaryPath, isCross }
}

export async function prepareCliSidecarAndCompletions(
  options: PrepareCliOptions,
): Promise<ResolveInfo> {
  const profile = options.profile ?? 'release'
  const exec = options.run ?? runCommand
  const targetFile = cliSidecarTargetFile(options.sidecarHost)
  const sidecarPath = path.join(options.sidecarDir, targetFile)

  await mkdir(options.sidecarDir, { recursive: true })

  let sourceBinary = options.prebuiltBinaryPath
  const plan = planCliCargoBuild({
    workspaceRoot: options.workspaceRoot,
    sidecarHost: options.sidecarHost,
    rustHost: options.rustHost,
    profile,
    platform: options.platform,
  })

  if (!sourceBinary) {
    options.debugLog?.(
      `building nyanpasu-cli with ${plan.command} ${plan.args.join(' ')}`,
    )
    const result = await exec(plan.command, {
      cwd: plan.cwd,
      args: plan.args,
      stdout: 'piped',
      stderr: 'piped',
    })
    if (!result.success) {
      throw new Error(
        `failed to build nyanpasu-cli (${result.code}):\n${result.stderr.toString('utf8')}`,
      )
    }
    sourceBinary = plan.targetBinaryPath
  }

  await copyFile(sourceBinary, sidecarPath)
  if (!options.sidecarHost.includes('windows')) {
    await chmod(sidecarPath, 0o755)
  }

  await generateCompletions({
    outputDir: options.completionsDir,
    workspaceRoot: options.workspaceRoot,
    executablePath:
      !plan.isCross && !options.prebuiltBinaryPath ? sidecarPath : undefined,
    profile,
    run: options.run,
  })

  return {
    file: targetFile,
    size: (await stat(sidecarPath)).size,
    cached: false,
  }
}
