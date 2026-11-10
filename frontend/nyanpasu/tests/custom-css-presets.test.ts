import { readdirSync, readFileSync } from 'node:fs'
import { describe, expect, test } from 'vitest'
import { CUSTOM_CSS_PRESETS } from '@/assets/custom-css-presets'
import { compileCustomCss } from '@/utils/custom-css-compiler'

const presetsDir = new URL('../src/assets/custom-css-presets/', import.meta.url)

// Vitest replaces a stylesheet imported with `?raw` by an empty string, so the
// sources are read from disk here.
const sources = readdirSync(presetsDir)
  .filter((file) => file.endsWith('.less'))
  .map(
    (file) => [file, readFileSync(new URL(file, presetsDir), 'utf8')] as const,
  )

describe('bundled custom CSS presets', () => {
  test('have unique ids', () => {
    const ids = CUSTOM_CSS_PRESETS.map((preset) => preset.id)

    expect(new Set(ids).size).toBe(ids.length)
  })

  test('register every stylesheet in the folder', () => {
    expect(CUSTOM_CSS_PRESETS.length).toBe(sources.length)
  })

  test.each(sources)('%s compiles to slot selectors', async (_, source) => {
    const css = await compileCustomCss(source)

    expect(css).toContain('[data-slot="')
    // A bare slot name left in a selector means the shorthand did not resolve.
    expect(css).not.toMatch(
      /(^|[\s>},])(app-|animated-tabs-|navbar-)[a-z-]+\s*[{,]/,
    )
  })
})
