import type { Highlighter } from 'shiki'
import { getSingletonHighlighterCore } from 'shiki/core'
import { createOnigurumaEngine } from 'shiki/engine/oniguruma'
import getWasm from 'shiki/wasm'
import { SYNTAX_THEME_NAME, syntaxTheme } from './shiki-theme'

let shiki: Highlighter | null = null

export async function getShikiSingleton() {
  if (!shiki) {
    shiki = (await getSingletonHighlighterCore({
      engine: createOnigurumaEngine(getWasm),
      themes: [syntaxTheme],
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
  return instance.codeToHtml(code, { lang, theme: SYNTAX_THEME_NAME })
}

export async function highlightJson(code: string) {
  const instance = await getShikiSingleton()
  await instance.loadLanguage(import('shiki/langs/json.mjs'))
  return instance.codeToHtml(code, { lang: 'json', theme: SYNTAX_THEME_NAME })
}

export async function highlightShell(code: string) {
  const instance = await getShikiSingleton()
  return instance.codeToHtml(code, { lang: 'shell', theme: SYNTAX_THEME_NAME })
}
