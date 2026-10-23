import { spawn, type ChildProcess } from 'node:child_process'
import { once } from 'node:events'
import { constants } from 'node:os'

export type StdioMode = 'piped' | 'inherit' | 'null'

export interface CommandOptions {
  args?: readonly string[]
  cwd?: string
  /** Added to the parent environment unless `clearEnv` is set. */
  env?: Record<string, string>
  clearEnv?: boolean
  signal?: AbortSignal
  stdin?: StdioMode
  /** Written to the child's stdin by `runCommand`, which then closes it. */
  input?: string | Uint8Array
  stdout?: StdioMode
  stderr?: StdioMode
}

export interface ExitStatus {
  success: boolean
  code: number
}

export interface CommandOutput extends ExitStatus {
  stdout: Buffer
  stderr: Buffer
}

const NODE_STDIO = {
  piped: 'pipe',
  inherit: 'inherit',
  null: 'ignore',
} as const

/** Start a child process. Rejects through `exitStatus` when it cannot start. */
export function spawnCommand(
  command: string,
  options: CommandOptions = {},
): ChildProcess {
  const {
    args = [],
    stdin = 'null',
    stdout = 'inherit',
    stderr = 'inherit',
  } = options
  return spawn(command, args, {
    cwd: options.cwd,
    env: options.clearEnv
      ? { ...options.env }
      : { ...process.env, ...options.env },
    signal: options.signal,
    stdio: [NODE_STDIO[stdin], NODE_STDIO[stdout], NODE_STDIO[stderr]],
  })
}

/** Wait for a child to finish; a signal exit maps to `128 + signal number`. */
export async function exitStatus(child: ChildProcess): Promise<ExitStatus> {
  const [code, signal] = (await once(child, 'close')) as [
    number | null,
    NodeJS.Signals | null,
  ]
  const resolved = code ?? 128 + (signal ? constants.signals[signal] : 0)
  return { success: resolved === 0, code: resolved }
}

/** Run a command to completion; output defaults to captured. */
export async function runCommand(
  command: string,
  options: CommandOptions = {},
): Promise<CommandOutput> {
  const { input, ...spawnOptions } = options
  const child = spawnCommand(command, {
    stdout: 'piped',
    stderr: 'piped',
    ...spawnOptions,
    stdin: input === undefined ? options.stdin : 'piped',
  })
  if (input !== undefined) {
    // The child may exit without reading its stdin; its exit status reports that.
    child.stdin!.on('error', () => {})
    child.stdin!.end(input)
  }
  const collect = async (stream: ChildProcess['stdout']) => {
    const chunks: Buffer[] = []
    for await (const chunk of stream ?? []) chunks.push(chunk as Buffer)
    return Buffer.concat(chunks)
  }
  const [status, stdout, stderr] = await Promise.all([
    exitStatus(child),
    collect(child.stdout),
    collect(child.stderr),
  ])
  return { ...status, stdout, stderr }
}
