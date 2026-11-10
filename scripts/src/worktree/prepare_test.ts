import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { pathToFileURL } from 'node:url'
import { initializeSubmodules } from './prepare.ts'

function git(cwd: string, ...args: string[]): string {
  return execFileSync('git', ['-c', 'protocol.file.allow=always', ...args], {
    cwd,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim()
}

function repo(path: string): void {
  mkdirSync(path)
  git(path, 'init')
  git(path, 'config', 'user.name', 'Test')
  git(path, 'config', 'user.email', 'test@example.com')
  writeFileSync(join(path, 'file'), 'original')
  git(path, 'add', 'file')
  git(path, 'commit', '-m', 'initial')
}

test('reuses nested submodules offline in independent checkouts', () => {
  const root = mkdtempSync(join(tmpdir(), 'worktree-prepare-'))
  try {
    const leaf = join(root, 'leaf')
    const parent = join(root, 'parent')
    const origin = join(root, 'origin')
    const source = join(root, 'source')
    const destination = join(root, 'destination')
    repo(leaf)
    repo(parent)
    git(parent, 'submodule', 'add', pathToFileURL(leaf).href, 'nested')
    git(parent, 'commit', '-am', 'add nested')
    repo(origin)
    git(origin, 'submodule', 'add', pathToFileURL(parent).href, 'module')
    git(origin, 'commit', '-am', 'add module')
    git(root, 'clone', origin, source)
    git(source, 'submodule', 'update', '--init', '--recursive')
    git(root, 'clone', origin, destination)
    rmSync(parent, { recursive: true, force: true })
    rmSync(leaf, { recursive: true, force: true })

    initializeSubmodules(destination, source)

    const targetFile = join(destination, 'module', 'nested', 'file')
    assert.equal(readFileSync(targetFile, 'utf8'), 'original')
    writeFileSync(targetFile, 'changed')
    assert.equal(
      readFileSync(join(source, 'module', 'nested', 'file'), 'utf8'),
      'original',
    )
    assert.equal(
      git(join(destination, 'module'), 'remote', 'get-url', 'origin'),
      pathToFileURL(parent).href,
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('uses the configured remote when the source submodule is absent', () => {
  const root = mkdtempSync(join(tmpdir(), 'worktree-prepare-'))
  try {
    const child = join(root, 'child')
    const source = join(root, 'source')
    const destination = join(root, 'destination')
    repo(child)
    repo(source)
    git(source, 'submodule', 'add', pathToFileURL(child).href, 'module')
    git(source, 'commit', '-am', 'add module')
    git(root, 'clone', source, destination)
    git(source, 'submodule', 'deinit', '-f', '--', 'module')
    // Local test remotes need the same explicit permission as any local Git URL.
    process.env.GIT_ALLOW_PROTOCOL = 'file'
    try {
      initializeSubmodules(destination, source)
    } finally {
      delete process.env.GIT_ALLOW_PROTOCOL
    }

    assert.equal(
      readFileSync(join(destination, 'module', 'file'), 'utf8'),
      'original',
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('fetches the pinned revision from the remote when the local source lacks it', () => {
  const root = mkdtempSync(join(tmpdir(), 'worktree-prepare-'))
  try {
    const child = join(root, 'child')
    const source = join(root, 'source')
    const destination = join(root, 'destination')
    repo(child)
    repo(source)
    git(source, 'submodule', 'add', pathToFileURL(child).href, 'module')
    git(source, 'commit', '-am', 'add module')
    writeFileSync(join(child, 'file'), 'new revision')
    git(child, 'commit', '-am', 'update child')
    const revision = git(child, 'rev-parse', 'HEAD')
    git(source, 'update-index', '--cacheinfo', `160000,${revision},module`)
    git(source, 'commit', '-m', 'update pin')
    git(root, 'clone', source, destination)
    const previous = process.env.GIT_ALLOW_PROTOCOL
    process.env.GIT_ALLOW_PROTOCOL = 'file'
    try {
      initializeSubmodules(destination, source)
    } finally {
      if (previous === undefined) delete process.env.GIT_ALLOW_PROTOCOL
      else process.env.GIT_ALLOW_PROTOCOL = previous
    }
    assert.equal(
      readFileSync(join(destination, 'module', 'file'), 'utf8'),
      'new revision',
    )
    assert.equal(
      readFileSync(join(source, 'module', 'file'), 'utf8'),
      'original',
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
