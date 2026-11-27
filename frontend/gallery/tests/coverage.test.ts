import { readFileSync } from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { getCoveredUiModules } from '../src/catalog/registry'

describe('UI Gallery specimen coverage', () => {
  it('covers every public module re-exported by @nyanpasu/ui/src/index.ts', () => {
    const uiIndexPath = path.resolve(
      import.meta.dirname,
      '../../ui/src/index.ts',
    )
    const uiIndexSource = readFileSync(uiIndexPath, 'utf8')
    const exportedModules = new Set<string>()

    for (const match of uiIndexSource.matchAll(
      /from\s+['"]\.\/([^'"]+)['"]/g,
    )) {
      exportedModules.add(match[1])
    }

    const coveredModules = new Set(getCoveredUiModules())
    const missing = [...exportedModules]
      .filter((mod) => !coveredModules.has(mod))
      .sort()

    expect(missing).toEqual([])
  })
})
