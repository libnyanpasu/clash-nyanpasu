import nyanpasuMergeSchema from 'meta-json-schema/schemas/clash-nyanpasu-merge-json-schema.json'
import clashMetaSchema from 'meta-json-schema/schemas/meta-json-schema.json'
import { configureMonacoYaml } from 'monaco-yaml'
import { OS } from '@/consts'
import { Monaco } from '@monaco-editor/react'

export const MONACO_FONT_FAMILY =
  '"Cascadia Code NF",' +
  '"Cascadia Code",' +
  'Fira Code,' +
  'JetBrains Mono,' +
  'Roboto Mono,' +
  '"Source Code Pro",' +
  'Consolas,' +
  'Menlo,' +
  'Monaco,' +
  'monospace,' +
  `${OS === 'windows' ? 'twemoji mozilla' : ''}`

let initd = false

export const beforeEditorMount = (monaco: Monaco) => {
  if (initd) {
    return
  }

  monaco.languages.typescript.javascriptDefaults.setCompilerOptions({
    target: monaco.languages.typescript.ScriptTarget.ES2020,
    allowNonTsExtensions: true,
    allowJs: true,
  })

  // console.log(clashMetaSchema)
  // console.log(nyanpasuMergeSchema)

  // configure yaml schema
  configureMonacoYaml(monaco, {
    validate: true,
    enableSchemaRequest: true,
    completion: true,
    schemas: [
      {
        uri: 'http://example.com/schema-name.json',
        fileMatch: ['**/*.clash.yaml'],
        // @ts-expect-error JSONSchema7 as JSONSchema
        schema: clashMetaSchema as JSONSchema7,
      },
      {
        uri: 'http://example.com/schema-name.json',
        fileMatch: ['**/*.merge.yaml'],
        // @ts-expect-error JSONSchema7 as JSONSchema
        schema: nyanpasuMergeSchema as JSONSchema7,
      },
    ],
  })

  // Register link provider for all supported languages
  const registerLinkProvider = (language: string) => {
    monaco.languages.registerLinkProvider(language, {
      provideLinks: (model, token) => {
        const links = []
        // More robust URL regex pattern
        const urlRegex = /\b(?:https?:\/\/|www\.)[^\s<>"']*[^<>\s"',.!?]/gi

        for (let i = 1; i <= model.getLineCount(); i++) {
          const line = model.getLineContent(i)
          let match

          while ((match = urlRegex.exec(line)) !== null) {
            const url = match[0].startsWith('http')
              ? match[0]
              : `https://${match[0]}`
            links.push({
              range: new monaco.Range(
                i,
                match.index + 1,
                i,
                match.index + match[0].length + 1,
              ),
              url,
            })
          }
        }

        return {
          links,
          dispose: () => {},
        }
      },
    })
  }

  // Register link provider for all languages we support
  registerLinkProvider('javascript')
  registerLinkProvider('lua')
  registerLinkProvider('yaml')

  initd = true
}
