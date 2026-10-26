import type { ColorScheme } from '@/hooks/use-color-scheme'
import TokensWorker from './yaml-tokens.worker?worker'

// The worker highlights this many lines in about a second, and holds up the
// documents asked for after them meanwhile. Larger documents stay plain text.
export const MAX_HIGHLIGHT_LINES = 100_000

/**
 * A document's tokens, packed so they cross from the worker without a copy.
 * The text stays with the caller, who sent it: a token is its length and
 * style, and its text is cut from the line when the line is drawn.
 */
export type DocumentTokens = {
  /** Where each line's tokens start, counted in tokens, and where the last ends. */
  lineStarts: Uint32Array
  /**
   * Two numbers per token: its length in UTF-16 code units, then its color's
   * index in `palette` shifted left four bits over its TextMate font style.
   */
  tokens: Uint32Array
  /** The colors the tokens use; an empty one leaves the text's own color. */
  palette: string[]
}

/** A piece of a line drawn in one style. */
export type LineToken = { text: string; color?: string; fontStyle: number }

/** The tokens of line `index`, whose text is `text`. */
export function lineTokens(
  document: DocumentTokens,
  index: number,
  text: string,
): LineToken[] {
  const { lineStarts, tokens, palette } = document
  const pieces: LineToken[] = []
  let offset = 0
  for (let token = lineStarts[index]; token < lineStarts[index + 1]; token++) {
    const length = tokens[token * 2]
    const style = tokens[token * 2 + 1]
    pieces.push({
      text: text.slice(offset, offset + length),
      color: palette[style >> 4] || undefined,
      fontStyle: style & 0xf,
    })
    offset += length
  }
  // A carriage return before the line break is no token's text.
  if (offset < text.length) {
    pieces.push({ text: text.slice(offset), fontStyle: 0 })
  }
  return pieces
}

export type TokenizeRequest = {
  id: number
  codes: readonly string[]
  scheme: ColorScheme
}

export type TokenizeResponse = { id: number } & (
  { documents: DocumentTokens[] } | { error: string }
)

let worker: Worker | undefined
let nextId = 0
const waiting = new Map<
  number,
  {
    resolve: (documents: DocumentTokens[]) => void
    reject: (error: Error) => void
  }
>()

function tokensWorker() {
  if (!worker) {
    worker = new TokensWorker()
    worker.onmessage = ({ data }: MessageEvent<TokenizeResponse>) => {
      const request = waiting.get(data.id)
      waiting.delete(data.id)
      if ('documents' in data) request?.resolve(data.documents)
      else request?.reject(new Error(data.error))
    }
    // A worker that cannot run leaves the documents plain.
    worker.onerror = (event) => {
      waiting.forEach(({ reject }) => reject(new Error(event.message)))
      waiting.clear()
    }
  }
  return worker
}

/**
 * Tokenizes YAML documents line by line in one color scheme, in a worker, for
 * renderers that lay out lines themselves. Both schemes at once would tokenize
 * the text once each. The documents go in one message: a message each costs a
 * round trip, which adds up for the hundreds of pieces of a diff.
 */
export function tokenizeYaml(codes: readonly string[], scheme: ColorScheme) {
  const id = (nextId += 1)
  return new Promise<DocumentTokens[]>((resolve, reject) => {
    waiting.set(id, { resolve, reject })
    tokensWorker().postMessage({ id, codes, scheme } satisfies TokenizeRequest)
  })
}
