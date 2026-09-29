import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import postcss from 'postcss'
import selectorParser from 'postcss-selector-parser'
import { expect, test } from 'vitest'
import postcssConfig from '../postcss.config.js'
import splitWhereCombinators from '../postcss/split-where-combinators.js'

const rewrite = async (css: string) =>
  (await postcss([splitWhereCombinators()]).process(css, { from: undefined }))
    .css

test.each([
  // Tailwind's space-* and divide-* output.
  [
    ':where(.space-y-2 > :not(:last-child)) {}',
    ':where(.space-y-2) > :where(:not(:last-child)) {}',
  ],
  [
    ':where(.dark\\:divide-outline-variant:where(.dark, .dark *) > :not(:last-child)) {}',
    ':where(.dark\\:divide-outline-variant:where(.dark, .dark *)) > :where(:not(:last-child)) {}',
  ],
  // Minified output, as in a production build.
  [
    ':where(.divide-x>:not(:last-child)){}',
    ':where(.divide-x) > :where(:not(:last-child)){}',
  ],
  // Every combinator, and each selector of a list on its own.
  [
    ':where(.a .b), :where(.a ~ .b), :where(.a + .b) {}',
    ':where(.a) :where(.b), :where(.a) ~ :where(.b), :where(.a) + :where(.b) {}',
  ],
  // Split at the last combinator only.
  [':where(.a > .b > .c) {}', ':where(.a > .b) > :where(.c) {}'],
  [
    '@layer utilities { :where(.space-x-1 > :not(:last-child)) {} }',
    '@layer utilities { :where(.space-x-1) > :where(:not(:last-child)) {} }',
  ],
])('splits %s', async (input, output) => {
  expect(await rewrite(input)).toBe(output)
})

test.each([
  // No combinator inside.
  ':where(.dark, .dark *) {}',
  ':where(code, kbd, pre, samp) {}',
  // Not the whole selector: splitting would have to move the other parts.
  '.x:where(.a > .b) {}',
  '.x > :where(.a > .b) {}',
  // More than one selector inside.
  ':where(.a, .b > .c) {}',
  // A pseudo-element cannot stand alone inside :where().
  ':where(.a > .b::after) {}',
])('leaves %s alone', async (input) => {
  expect(await rewrite(input)).toBe(input)
})

// The app's stylesheet through the app's PostCSS pipeline must not keep a
// combinator inside :where() in front of a positional pseudo-class: WebKit
// then checks the pseudo-class on every element and restyles whole subtrees
// on DOM insertions.
test('the app stylesheet has no positional subject hidden in :where()', async () => {
  const from = fileURLToPath(
    new URL('../src/assets/styles/tailwind.css', import.meta.url),
  )
  const result = await postcss(postcssConfig.plugins).process(
    await readFile(from, 'utf8'),
    { from },
  )

  const selectors: string[] = []
  result.root.walkRules((rule) => {
    selectors.push(...rule.selectors)
  })

  // Sanity check that Tailwind generated the utilities this test is about.
  expect(selectors.some((selector) => selector.includes('.space-y-2'))).toBe(
    true,
  )

  const positional = /:(first-child|last-child|only-child|nth-)/
  const hidden = selectors.filter((selector) => {
    let found = false
    selectorParser((root) => {
      root.walkPseudos((pseudo) => {
        if (pseudo.value !== ':where') {
          return
        }

        for (const inner of pseudo.nodes) {
          if (
            inner.some((node) => node.type === 'combinator') &&
            positional.test(inner.toString())
          ) {
            found = true
          }
        }
      })
    }).processSync(selector)
    return found
  })
  expect(hidden).toEqual([])
}, 30_000)
