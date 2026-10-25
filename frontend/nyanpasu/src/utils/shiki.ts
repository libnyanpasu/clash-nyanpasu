import type { Highlighter } from 'shiki'
import { getSingletonHighlighterCore } from 'shiki/core'
import { createOnigurumaEngine } from 'shiki/engine/oniguruma'
import minLight from 'shiki/themes/min-light.mjs'
import nord from 'shiki/themes/nord.mjs'
import getWasm from 'shiki/wasm'

let shiki: Highlighter | null = null

export async function getShikiSingleton() {
  if (!shiki) {
    shiki = (await getSingletonHighlighterCore({
      engine: createOnigurumaEngine(getWasm),
      themes: [nord, minLight],
      langs: [() => import('shiki/langs/shell.mjs')],
    })) as Highlighter
  }
  return shiki
}

const shellGrammar = () => import('shiki/langs/shell.mjs')
const yamlGrammar = () => import('shiki/langs/yaml.mjs')

const MARKDOWN_CODE_GRAMMARS = {
  bash: shellGrammar,
  diff: () => import('shiki/langs/diff.mjs'),
  js: () => import('shiki/langs/javascript.mjs'),
  json: () => import('shiki/langs/json.mjs'),
  rust: () => import('shiki/langs/rust.mjs'),
  sh: shellGrammar,
  shell: shellGrammar,
  toml: () => import('shiki/langs/toml.mjs'),
  ts: () => import('shiki/langs/typescript.mjs'),
  tsx: () => import('shiki/langs/tsx.mjs'),
  yaml: yamlGrammar,
  yml: yamlGrammar,
} as const

/** Highlights a fenced Markdown code block; `null` for an unsupported language. */
export async function highlightMarkdownCode(code: string, lang: string) {
  if (!Object.hasOwn(MARKDOWN_CODE_GRAMMARS, lang)) return null
  const instance = await getShikiSingleton()
  await instance.loadLanguage(
    MARKDOWN_CODE_GRAMMARS[lang as keyof typeof MARKDOWN_CODE_GRAMMARS](),
  )
  return instance.codeToHtml(code, {
    lang,
    themes: { light: 'min-light', dark: 'nord' },
  })
}

export async function highlightJson(code: string) {
  const instance = await getShikiSingleton()
  await instance.loadLanguage(import('shiki/langs/json.mjs'))
  return instance.codeToHtml(code, {
    lang: 'json',
    themes: { light: 'min-light', dark: 'nord' },
  })
}
