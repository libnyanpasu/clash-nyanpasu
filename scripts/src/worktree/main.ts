import { execSync } from 'node:child_process'
import { realpathSync } from 'node:fs'
import { resolve } from 'node:path'
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts'
import { initializeSubmodules } from './prepare.ts'

const source = process.argv[2]
if (!source || process.argv.length !== 3) {
  console.error('Usage: pnpm run prepare:worktree <existing-checkout>')
  process.exit(1)
}
const sourceRoot = resolve(source)
if (realpathSync(sourceRoot) === realpathSync(WORKSPACE_ROOT)) {
  throw new Error('The source and destination checkouts must be different')
}
initializeSubmodules(WORKSPACE_ROOT, sourceRoot)
execSync('pnpm install --frozen-lockfile --prefer-offline', {
  cwd: WORKSPACE_ROOT,
  stdio: 'inherit',
})
