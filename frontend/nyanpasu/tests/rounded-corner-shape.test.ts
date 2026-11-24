import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import postcss from 'postcss'
import { expect, test } from 'vitest'
import postcssConfig from '../postcss.config.js'

test('the app stylesheet applies superellipse(2) to rounded elements while keeping rounded-full round', async () => {
  const from = fileURLToPath(
    new URL('../src/assets/styles/tailwind.css', import.meta.url),
  )
  const result = await postcss(postcssConfig.plugins).process(
    await readFile(from, 'utf8'),
    { from },
  )

  const declsBySelector = new Map<string, Map<string, string>>()

  result.root.walkRules((rule) => {
    const map = declsBySelector.get(rule.selector) ?? new Map<string, string>()

    rule.each((node) => {
      if (node.type === 'decl') {
        map.set(node.prop, node.value)
      }
    })

    declsBySelector.set(rule.selector, map)
  })

  expect(declsBySelector.get("[class*='rounded']")?.get('corner-shape')).toBe(
    'superellipse(2)',
  )
  expect(declsBySelector.get('.rounded-full')?.get('corner-shape')).toBe(
    'round',
  )
  expect(declsBySelector.get('.rounded-none')?.get('corner-shape')).toBe(
    'round',
  )
  expect(
    declsBySelector.get('.rounded-l-full')?.get('corner-top-left-shape'),
  ).toBe('round')
  expect(
    declsBySelector.get('.rounded-r-full')?.get('corner-top-right-shape'),
  ).toBe('round')
}, 30_000)
