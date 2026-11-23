import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import postcss from 'postcss'
import { expect, test } from 'vitest'
import postcssConfig from '../postcss.config.js'
import roundedCornerShape from '../postcss/rounded-corner-shape.js'

const transform = async (css: string) =>
  (await postcss([roundedCornerShape()]).process(css, { from: undefined })).css

test.each([
  [
    '.rounded { border-radius: 0.25rem; }',
    '.rounded { border-radius: 0.25rem; corner-shape: superellipse(2); }',
  ],
  [
    '.rounded-xl { border-radius: var(--radius-xl); }',
    '.rounded-xl { border-radius: var(--radius-xl); corner-shape: superellipse(2); }',
  ],
  [
    '.rounded-3xl\\! { border-radius: var(--radius-3xl) !important; }',
    '.rounded-3xl\\! { border-radius: var(--radius-3xl) !important; corner-shape: superellipse(2) !important; }',
  ],
  [
    '.hover\\:rounded-lg:hover { border-radius: var(--radius-lg); }',
    '.hover\\:rounded-lg:hover { border-radius: var(--radius-lg); corner-shape: superellipse(2); }',
  ],
  [
    '.rounded-\\[20px\\] { border-radius: 20px; }',
    '.rounded-\\[20px\\] { border-radius: 20px; corner-shape: superellipse(2); }',
  ],
  [
    '.rounded-\\[inherit\\] { border-radius: inherit; }',
    '.rounded-\\[inherit\\] { border-radius: inherit; corner-shape: inherit; }',
  ],
  [
    '.rounded-full { border-radius: calc(infinity * 1px); }',
    '.rounded-full { border-radius: calc(infinity * 1px); corner-shape: round; }',
  ],
  [
    '.rounded-none { border-radius: 0px; }',
    '.rounded-none { border-radius: 0px; corner-shape: round; }',
  ],
  [
    '.rounded-t-lg { border-top-left-radius: var(--radius-lg); border-top-right-radius: var(--radius-lg); }',
    '.rounded-t-lg { border-top-left-radius: var(--radius-lg); border-top-right-radius: var(--radius-lg); corner-top-left-shape: superellipse(2); corner-top-right-shape: superellipse(2); }',
  ],
  [
    '.rounded-l-full { border-top-left-radius: calc(infinity * 1px); border-bottom-left-radius: calc(infinity * 1px); }',
    '.rounded-l-full { border-top-left-radius: calc(infinity * 1px); border-bottom-left-radius: calc(infinity * 1px); corner-top-left-shape: round; corner-bottom-left-shape: round; }',
  ],
])('patches %s', async (input, output) => {
  expect(await transform(input)).toBe(output)
})

test.each([
  '.card { border-radius: 12px; }',
  '.rounded-xl { border-radius: var(--radius-xl); corner-shape: round; }',
  '.not-rounded { color: red; }',
])('leaves %s alone', async (input) => {
  expect(await transform(input)).toBe(input)
})

test('the app stylesheet patches rounded utilities with superellipse(2) while keeping rounded-full round', async () => {
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

  expect(declsBySelector.get('.rounded-xl')?.get('corner-shape')).toBe(
    'superellipse(2)',
  )
  expect(declsBySelector.get('.rounded-full')?.get('corner-shape')).toBe(
    'round',
  )
  expect(declsBySelector.get('.rounded-none')?.get('corner-shape')).toBe(
    'round',
  )
  expect(
    declsBySelector.get('.rounded-r-xs')?.get('corner-top-right-shape'),
  ).toBe('superellipse(2)')
  expect(
    declsBySelector.get('.rounded-l-full')?.get('corner-top-left-shape'),
  ).toBe('round')
}, 30_000)
