import selectorParser from 'postcss-selector-parser'

// Rewrites `:where(A > B)` into `:where(A) > :where(B)`.
//
// Both forms have zero specificity and match the same elements. WebKit,
// however, can only skip a rule early through its ancestor filter when the
// combinator is outside `:where()`. With the combinator inside, it checks the
// subject against every element, and a positional subject such as
// `:not(:last-child)` (Tailwind's space-* and divide-* utilities) then flags
// every parent, so each append restyles the previous last child's whole
// subtree. See docs/reviews/2026-09-30-webkit-positional-selector-style-invalidation.md.

/** @param {import('postcss-selector-parser').Selector} selector */
function splitAtLastCombinator(selector) {
  const nodes = selector.nodes
  let index = -1

  for (let i = nodes.length - 1; i >= 0; i--) {
    if (nodes[i].type === 'combinator') {
      index = i
      break
    }
  }

  if (index === -1) {
    return null
  }

  return {
    left: nodes.slice(0, index),
    combinator: nodes[index],
    right: nodes.slice(index + 1),
  }
}

/** @param {import('postcss-selector-parser').Node[]} nodes */
function wrapInWhere(nodes) {
  const selector = selectorParser.selector({ value: '' })

  for (const node of nodes) {
    selector.append(node.clone())
  }

  selector.first.spaces.before = ''
  selector.last.spaces.after = ''

  const where = selectorParser.pseudo({ value: ':where' })
  where.append(selector)
  return where
}

/** @param {import('postcss-selector-parser').Selector} selector */
function rewriteSelector(selector) {
  // Only a selector that is exactly one `:where()` holding one complex
  // selector: that is the shape Tailwind emits, and the only one where the
  // split is a plain textual equivalence.
  if (selector.nodes.length !== 1) {
    return
  }

  const where = selector.first

  if (
    where.type !== 'pseudo' ||
    where.value !== ':where' ||
    where.nodes.length !== 1
  ) {
    return
  }

  const inner = where.first
  const parts = splitAtLastCombinator(inner)

  // A pseudo-element cannot sit inside `:where()` on its own.
  if (
    !parts ||
    parts.right.some(
      (node) => node.type === 'pseudo' && node.value.startsWith('::'),
    )
  ) {
    return
  }

  const combinator = parts.combinator.clone()
  // A descendant combinator is the whitespace itself; others get spaces.
  if (combinator.value.trim() !== '') {
    combinator.spaces.before = ' '
    combinator.spaces.after = ' '
  }

  const left = wrapInWhere(parts.left)
  const right = wrapInWhere(parts.right)
  left.spaces.before = where.spaces.before
  right.spaces.after = where.spaces.after

  selector.removeAll()
  selector.append(left)
  selector.append(combinator)
  selector.append(right)
}

const processor = selectorParser((selectors) => {
  selectors.each(rewriteSelector)
})

/** @type {import('postcss').PluginCreator<void>} */
const splitWhereCombinators = () => ({
  postcssPlugin: 'split-where-combinators',
  Rule(rule) {
    if (!rule.selector.includes(':where(')) {
      return
    }

    rule.selector = processor.processSync(rule.selector)
  },
})

splitWhereCombinators.postcss = true

export default splitWhereCombinators
