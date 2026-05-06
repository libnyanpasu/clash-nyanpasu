/**
 * Monaco CSS editor helpers for Clash Nyanpasu custom CSS.
 * Registers a completion item provider that suggests data-slot values.
 */

// Ensure CSS basic language support is loaded
import 'monaco-editor/esm/vs/basic-languages/css/css.contribution.js'
import 'monaco-editor/esm/vs/language/css/monaco.contribution.js'
import { DATA_SLOTS } from '@/generated/data-slots.gen'
import type { Monaco } from '@monaco-editor/react'

let registered = false

/**
 * Register a CSS completion provider for data-slot selectors.
 * Must be called with the monaco instance from @monaco-editor/react's beforeMount.
 * Safe to call multiple times — only registers once.
 */
export function registerCssDataSlotCompletion(monacoInstance: Monaco): void {
  if (registered) return
  registered = true

  monacoInstance.languages.registerCompletionItemProvider('css', {
    triggerCharacters: ['"', '=', '-', '['],

    provideCompletionItems(
      model: Monaco['editor']['ITextModel'],
      position: Monaco['Position'],
    ) {
      const textBefore = model
        .getLineContent(position.lineNumber)
        .substring(0, position.column - 1)

      // Check if we're in an attribute selector context: [data-slot=" or [data-slot=
      const attrMatch = textBefore.match(/\[data-slot="?([^"\]]*)$/)
      const attrPrefix = attrMatch?.[1]

      const isAttrContext = attrPrefix !== undefined

      // For direct slot selector context, only suggest if we're likely in selector position
      let directPrefix: string | undefined
      if (!isAttrContext) {
        const directMatch = textBefore.match(/(^|[\s>+~,])([a-zA-Z0-9_-]*)$/)
        directPrefix = directMatch?.[2]

        // Light heuristic: if we're inside a declarations block (after `{`), don't suggest
        if (!isLikelySelectorPosition(model, position)) {
          directPrefix = undefined
        }
      }

      const prefix = isAttrContext ? attrPrefix : directPrefix
      if (prefix === undefined) return { suggestions: [] }

      const range = new monacoInstance.Range(
        position.lineNumber,
        position.column - prefix.length,
        position.lineNumber,
        position.column,
      )

      const suggestions = DATA_SLOTS.filter((s) => s.startsWith(prefix)).map(
        (slot) => ({
          label: slot,
          kind: monacoInstance.languages.CompletionItemKind.Value,
          // Attribute selector: complete with closing "]
          // Direct selector: just the slot name
          insertText: isAttrContext ? `${slot}"]` : slot,
          range,
          detail: 'data-slot',
          documentation: {
            value: `Selects: \`[data-slot="${slot}"]\``,
          },
        }),
      )

      return { suggestions }
    },
  })
}

/**
 * Heuristic: scan backwards from the cursor to find whether we're likely
 * in a selector position (before a `{`) rather than inside a declarations block.
 * Returns true if the nearest `{` or `}` found (going left) is a `}`, or if
 * nothing is found (top of file) — i.e., we're at the top level.
 */
function isLikelySelectorPosition(
  model: { getOffsetAt: (p: unknown) => number; getValue: () => string },
  position: unknown,
): boolean {
  const offset = model.getOffsetAt(position)
  const text = model.getValue()

  for (let i = offset - 1; i >= 0; i--) {
    const ch = text[i]
    if (ch === '}') return true // after a closing brace → selector position
    if (ch === '{') return false // inside a block → declaration position
  }
  return true // top of file → selector position
}
