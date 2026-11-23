// Adds CSS `corner-shape: superellipse(2)` (and corner-specific longhands)
// to Tailwind `rounded-*` utilities so rounded corners render with continuous
// superellipse curves on supporting engines (e.g. Chromium 139+), while
// keeping `rounded-full` and `rounded-none` as `round`.

const RADIUS_TO_CORNER_SHAPE = {
  'border-radius': 'corner-shape',
  'border-top-left-radius': 'corner-top-left-shape',
  'border-top-right-radius': 'corner-top-right-shape',
  'border-bottom-right-radius': 'corner-bottom-right-shape',
  'border-bottom-left-radius': 'corner-bottom-left-shape',
  'border-start-start-radius': 'corner-start-start-shape',
  'border-start-end-radius': 'corner-start-end-shape',
  'border-end-start-radius': 'corner-end-start-shape',
  'border-end-end-radius': 'corner-end-end-shape',
}

const ROUNDED_SELECTOR_RE =
  /(?:^|[^\w-])\.(?:[^.\s]+\\:)?rounded(?=$|[-\\:\s,>+~.[)!])/

const FULL_RADIUS_RE =
  /^(?:calc\(infinity\s*\*\s*1px\)|9999px|2147483647px|50%|100%)$/i

const ZERO_RADIUS_RE = /^0(?:[a-z%]+)?$/i

/** @param {string} value */
function resolveCornerShapeValue(value) {
  const normalized = value.trim()

  if (
    normalized === 'inherit' ||
    normalized === 'initial' ||
    normalized === 'unset'
  ) {
    return normalized
  }

  if (FULL_RADIUS_RE.test(normalized) || ZERO_RADIUS_RE.test(normalized)) {
    return 'round'
  }

  return 'superellipse(2)'
}

/** @type {import('postcss').PluginCreator<void>} */
const roundedCornerShape = () => ({
  postcssPlugin: 'rounded-corner-shape',
  Rule(rule) {
    if (!ROUNDED_SELECTOR_RE.test(rule.selector)) {
      return
    }

    /** @type {Set<string>} */
    const existingProps = new Set()
    /** @type {Array<{ prop: string; value: string; important: boolean }>} */
    const additions = []

    rule.each((node) => {
      if (node.type !== 'decl') {
        return
      }

      existingProps.add(node.prop)

      const cornerShapeProp = RADIUS_TO_CORNER_SHAPE[node.prop]

      if (!cornerShapeProp) {
        return
      }

      additions.push({
        prop: cornerShapeProp,
        value: resolveCornerShapeValue(node.value),
        important: node.important,
      })
    })

    for (const addition of additions) {
      if (existingProps.has(addition.prop)) {
        continue
      }

      existingProps.add(addition.prop)
      rule.append(addition)
    }
  },
})

roundedCornerShape.postcss = true

export default roundedCornerShape
