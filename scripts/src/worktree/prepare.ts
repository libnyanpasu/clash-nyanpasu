import { execFileSync, spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

function git(cwd: string, args: string[]): string {
  return execFileSync('git', args, { cwd, encoding: 'utf8' }).trim()
}

export function initializeSubmodules(
  destination: string,
  source: string,
): void {
  const modules = join(destination, '.gitmodules')
  if (!existsSync(modules)) return

  const result = spawnSync(
    'git',
    [
      'config',
      '--null',
      '--file',
      modules,
      '--get-regexp',
      '^submodule\\..*\\.path$',
    ],
    { cwd: destination, encoding: 'utf8' },
  )
  if (result.status === 1) return
  if (result.status !== 0)
    throw new Error(result.stderr || 'Cannot read .gitmodules')

  for (const entry of result.stdout.split('\0').filter(Boolean)) {
    const separator = entry.indexOf('\n')
    const key = entry.slice(0, separator)
    const path = entry.slice(separator + 1)
    const url = git(destination, [
      'config',
      '--file',
      modules,
      '--get',
      `${key.slice(0, -5)}.url`,
    ])
    const sourcePath = resolve(source, path)
    const destinationPath = resolve(destination, path)
    let sourceGitDir: string | undefined
    try {
      if (git(sourcePath, ['rev-parse', '--show-prefix']) === '') {
        sourceGitDir = git(sourcePath, ['rev-parse', '--absolute-git-dir'])
      }
    } catch {
      // An uninitialized source submodule has no local object database to reuse.
    }

    const update = ['submodule', 'update', '--init', '--', path]
    if (sourceGitDir) {
      console.log(`Preparing ${destinationPath} from ${sourceGitDir}`)
      const localUrl = pathToFileURL(sourceGitDir).href
      const local = spawnSync(
        'git',
        [
          '-c',
          'protocol.file.allow=always',
          '-c',
          `url.${localUrl}.insteadOf=${url}`,
          ...update,
        ],
        {
          cwd: destination,
          stdio: 'inherit',
        },
      )
      if (local.status !== 0) {
        console.warn(
          `Local objects unavailable for ${path}; trying its configured remote.`,
        )
        execFileSync('git', update, { cwd: destination, stdio: 'inherit' })
      }
    } else {
      console.log(
        `No initialized local source for ${path}; using its configured remote.`,
      )
      execFileSync('git', update, { cwd: destination, stdio: 'inherit' })
    }
    initializeSubmodules(destinationPath, sourcePath)
  }
}
