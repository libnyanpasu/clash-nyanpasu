/**
 * Custom CSS compiler for Clash Nyanpasu.
 *
 * Compiles user CSS shorthand into standard CSS:
 *   - `slot-name { }` -> `[data-slot="slot-name"] { }`
 *   - `[data-slot=slot-name] { }` -> `[data-slot="slot-name"] { }`
 *
 * Only known slots (from DATA_SLOTS) are transformed as bare selectors.
 * Unknown bare identifiers are left untouched to preserve normal CSS semantics.
 */

import { DATA_SLOTS } from '@/generated/data-slots.gen'

const SLOT_SET = new Set<string>(DATA_SLOTS)

// Build a pattern that matches longest slots first to avoid partial matches.
const SLOT_PATTERN = [...DATA_SLOTS]
  .sort((a, b) => b.length - a.length)
  .map((slot) => slot.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
  .join('|')

// Matches a known slot name when used as a bare selector token.
const SLOT_SELECTOR_RE = new RegExp(
  `(^|[\\s>+~,])(${SLOT_PATTERN})(?=$|[\\s>+~.#:\\[{,])`,
  'g',
)

/**
 * Compile user CSS source into standard CSS ready for injection into the DOM.
 * The original source is preserved in KV storage; only the compiled result is injected.
 */
export function compileCustomCss(source: string): string {
  return twoPassCompile(source, compileSelectorList)
}

function compileSelectorList(selectorList: string): string {
  return splitSelectors(selectorList)
    .map((selector) =>
      selector
        // Normalize [data-slot=slot-name] -> [data-slot="slot-name"]
        .replace(
          /\[data-slot=([a-zA-Z0-9_-]+)\]/g,
          (_, slot: string) => `[data-slot="${slot}"]`,
        )
        // Compile direct bare slot selectors: slot-name -> [data-slot="slot-name"]
        .replace(SLOT_SELECTOR_RE, (_, prefix: string, slot: string) =>
          SLOT_SET.has(slot)
            ? `${prefix}[data-slot="${slot}"]`
            : `${prefix}${slot}`,
        ),
    )
    .join(', ')
}

/**
 * Split a CSS selector list by commas, respecting parentheses and brackets.
 * E.g. `:is(a, b), c` -> [`:is(a, b)`, ` c`]
 */
function splitSelectors(selectorList: string): string[] {
  const selectors: string[] = []
  let depth = 0
  let start = 0

  for (let i = 0; i < selectorList.length; i++) {
    const ch = selectorList[i]
    if (ch === '(' || ch === '[') depth++
    else if (ch === ')' || ch === ']') depth--
    else if (ch === ',' && depth === 0) {
      selectors.push(selectorList.slice(start, i))
      start = i + 1
    }
  }
  selectors.push(selectorList.slice(start))
  return selectors
}

function isContainerAtRule(s: string): boolean {
  return /^@(media|supports|layer|container)\b/.test(s)
}

function isKeyframesAtRule(s: string): boolean {
  return /^@(-\w+-)?keyframes\b/.test(s)
}

type Token =
  | { kind: 'text'; value: string }
  | { kind: 'open' }
  | { kind: 'close' }

/**
 * Tokenize a CSS source string into text segments, `{`, and `}` tokens,
 * preserving string literals and comments verbatim.
 */
function tokenize(source: string): Token[] {
  const tokens: Token[] = []
  let i = 0
  const len = source.length
  let text = ''

  const flushText = () => {
    if (text) {
      tokens.push({ kind: 'text', value: text })
      text = ''
    }
  }

  while (i < len) {
    const ch = source[i]

    // String literals
    if (ch === '"' || ch === "'") {
      const quote = ch
      let s = ch
      i++
      while (i < len) {
        const c = source[i]
        if (c === '\\') {
          s += c
          i++
          if (i < len) {
            s += source[i]
            i++
          }
          continue
        }
        s += c
        i++
        if (c === quote) break
      }
      text += s
      continue
    }

    // Comments
    if (ch === '/' && source[i + 1] === '*') {
      const end = source.indexOf('*/', i + 2)
      if (end === -1) {
        text += source.slice(i)
        i = len
        break
      }
      text += source.slice(i, end + 2)
      i = end + 2
      continue
    }

    if (ch === '{') {
      flushText()
      tokens.push({ kind: 'open' })
      i++
      continue
    }
    if (ch === '}') {
      flushText()
      tokens.push({ kind: 'close' })
      i++
      continue
    }

    text += ch
    i++
  }
  flushText()
  return tokens
}

/**
 * Rebuild CSS from tokens, transforming selector candidates.
 * A "selector candidate" is a text token immediately followed by `{`.
 * Container at-rules nest further rules; keyframes skip transformation.
 */
function twoPassCompile(
  source: string,
  transform: (selectorList: string) => string,
): string {
  const tokens = tokenize(source)

  // Coalesce consecutive text tokens
  const coalesced: Token[] = []
  for (const tok of tokens) {
    const last = coalesced[coalesced.length - 1]
    if (tok.kind === 'text' && last?.kind === 'text') {
      ;(last as { kind: 'text'; value: string }).value += tok.value
    } else {
      coalesced.push({ ...tok } as Token)
    }
  }

  // containerStack[depth] === true: this block is a container (can have nested rules)
  // containerStack[depth] === false: this block is a declarations block (e.g. keyframes body or plain rule)
  const containerStack: boolean[] = []

  let out = ''

  for (let j = 0; j < coalesced.length; j++) {
    const tok = coalesced[j]

    if (tok.kind === 'text') {
      const next = coalesced[j + 1]
      if (next?.kind === 'open') {
        // This text precedes `{`: it's a selector or at-rule preamble
        const candidate = tok.value
        const trimmed = candidate.trim()

        if (isKeyframesAtRule(trimmed)) {
          out += candidate
          containerStack.push(false) // keyframes: inner content is not selector-based
        } else if (isContainerAtRule(trimmed)) {
          out += candidate
          containerStack.push(true) // container: inner rules have selectors
        } else {
          // Regular rule or keyframe offset
          const depth = containerStack.length
          const inKeyframes = depth > 0 && !containerStack[depth - 1]

          if (!inKeyframes) {
            out += transformSelectorCandidate(candidate, transform)
          } else {
            out += candidate // inside @keyframes, don't transform "from", "to", etc.
          }
          containerStack.push(false) // declarations block
        }
      } else {
        out += tok.value
      }
    } else if (tok.kind === 'open') {
      out += '{'
    } else if (tok.kind === 'close') {
      out += '}'
      containerStack.pop()
    }
  }

  return out
}

function transformSelectorCandidate(
  candidate: string,
  transform: (selectorList: string) => string,
): string {
  const leading = /^(\s*)/.exec(candidate)?.[1] ?? ''
  const trailing = /(\s*)$/.exec(candidate)?.[1] ?? ''
  const trimmed = candidate.trim()
  if (!trimmed) return candidate
  return leading + transform(trimmed) + trailing
}
