import { execSync } from 'child_process'

export const GIT_SHORT_HASH = execSync('git rev-parse --short HEAD')
  .toString()
  .trim()
