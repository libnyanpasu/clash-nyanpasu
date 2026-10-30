import type { ThemedToken } from 'shiki'
import { createHighlighterCore } from 'shiki/core'
import { createOnigurumaEngine } from 'shiki/engine/oniguruma'
import yaml from 'shiki/langs/yaml.mjs'
import minLight from 'shiki/themes/min-light.mjs'
import nord from 'shiki/themes/nord.mjs'
import getWasm from 'shiki/wasm'
import type {
  DocumentTokens,
  TokenizeRequest,
  TokenizeResponse,
} from './yaml-tokens'

// Tokenizes YAML off the main thread: a document of a few thousand lines
// takes tens of milliseconds, and twenty thousand some two hundred.
const highlighter = createHighlighterCore({
  engine: createOnigurumaEngine(getWasm),
  themes: [minLight, nord],
  langs: [yaml],
})

const SCHEME_THEMES = { light: 'min-light', dark: 'nord' } as const

function pack(lines: ThemedToken[][]): DocumentTokens {
  const palette: string[] = []
  const colors = new Map<string, number>()
  const lineStarts = new Uint32Array(lines.length + 1)
  const tokens = new Uint32Array(
    lines.reduce((count, line) => count + line.length, 0) * 2,
  )
  let token = 0
  lines.forEach((line, index) => {
    lineStarts[index] = token
    for (const { content, color = '', fontStyle = 0 } of line) {
      let colorIndex = colors.get(color)
      if (colorIndex === undefined) {
        colorIndex = palette.push(color) - 1
        colors.set(color, colorIndex)
      }
      tokens[token * 2] = content.length
      tokens[token * 2 + 1] =
        (colorIndex << 4) | (fontStyle > 0 ? fontStyle : 0)
      token++
    }
  })
  lineStarts[lines.length] = token
  return { lineStarts, tokens, palette }
}

self.onmessage = async ({ data }: MessageEvent<TokenizeRequest>) => {
  let response: TokenizeResponse
  try {
    const instance = await highlighter
    const documents = data.codes.map((code) =>
      pack(
        instance.codeToTokens(code, {
          lang: 'yaml',
          theme: SCHEME_THEMES[data.scheme],
        }).tokens,
      ),
    )
    response = { id: data.id, documents }
  } catch (error) {
    response = { id: data.id, error: String(error) }
  }
  // The packed tokens move to the main thread rather than being copied.
  self.postMessage(response, {
    transfer:
      'documents' in response
        ? response.documents.flatMap(({ lineStarts, tokens }) => [
            lineStarts.buffer,
            tokens.buffer,
          ])
        : [],
  })
}
