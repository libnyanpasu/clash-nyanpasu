import { OS } from '@/consts'
import '@/services/monaco'
import { useAtomValue } from 'jotai'
import { type JSONSchema7 } from 'json-schema'
import nyanpasuMergeSchema from 'meta-json-schema/schemas/clash-nyanpasu-merge-json-schema.json'
import clashMetaSchema from 'meta-json-schema/schemas/meta-json-schema.json'
import { type editor } from 'monaco-editor'
import { configureMonacoYaml } from 'monaco-yaml'
import { nanoid } from 'nanoid'
import { useCallback, useMemo, useRef } from 'react'
// schema
import { themeMode } from '@/store'
import MonacoEditor, { type Monaco } from '@monaco-editor/react'
import { openThat } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'

export interface ProfileMonacoViewProps {
  value?: string
  onChange?: (value: string) => void
  language?: string
  className?: string
  readonly?: boolean
  schemaType?: 'clash' | 'merge'
  onValidate?: (markers: editor.IMarker[]) => void
}

export interface ProfileMonacoViewRef {
  getValue: () => string | undefined
}

let initd = false

export const beforeEditorMount = (monaco: Monaco) => {
  if (!initd) {
    monaco.languages.typescript.javascriptDefaults.setCompilerOptions({
      target: monaco.languages.typescript.ScriptTarget.ES2020,
      allowNonTsExtensions: true,
      allowJs: true,
    })
    console.log(clashMetaSchema)
    console.log(nyanpasuMergeSchema)
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
  }
  initd = true
}

export default function ProfileMonacoViewer({
  value,
  language,
  readonly = false,
  schemaType,
  className,
  onValidate,
  ...others
}: ProfileMonacoViewProps) {
  const mode = useAtomValue(themeMode)

  const path = useMemo(
    () => `${nanoid()}.${schemaType ? `${schemaType}.` : ''}${language}`,
    [schemaType, language],
  )

  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null)

  const onChange = useCallback(
    (value: string | undefined) => {
      if (value && others.onChange) {
        others.onChange(value)
      }
    },
    [others],
  )

  const handleEditorDidMount = useCallback(
    (editor: editor.IStandaloneCodeEditor, monaco: Monaco) => {
      editorRef.current = editor

      // Enable URL detection and handling
      editor.onMouseDown((e) => {
        const position = e.target.position
        if (!position) return

        // Get the model
        const model = editor.getModel()
        if (!model) return

        // Get the word at the clicked position
        const wordAtPosition = model.getWordAtPosition(position)
        if (!wordAtPosition) return

        // More comprehensive URL regex pattern
        const urlRegex = /\b(?:https?:\/\/|www\.)[^\s<>"']*[^<>\s"',.!?]/gi

        // Check if the clicked word is part of a URL
        const lineContent = model.getLineContent(position.lineNumber)
        let match

        while ((match = urlRegex.exec(lineContent)) !== null) {
          const urlStart = match.index + 1
          const urlEnd = urlStart + match[0].length

          // Check if the click position is within the URL
          if (position.column >= urlStart && position.column <= urlEnd) {
            // Only handle Ctrl+Click or Cmd+Click
            if (e.event.ctrlKey || e.event.metaKey) {
              // Add protocol if missing (for www. URLs)
              const url = match[0].startsWith('http')
                ? match[0]
                : `https://${match[0]}`
              openThat(url)
              e.event.preventDefault()
              break
            }
          }
        }
      })
    },
    [],
  )

  return (
    <MonacoEditor
      className={cn(className)}
      value={value}
      language={language}
      path={path}
      theme={mode === 'light' ? 'vs' : 'vs-dark'}
      beforeMount={beforeEditorMount}
      onMount={handleEditorDidMount}
      onChange={onChange}
      onValidate={onValidate}
      options={{
        readOnly: readonly,
        mouseWheelZoom: true,
        renderValidationDecorations: 'on',
        tabSize: language === 'yaml' ? 2 : 4,
        minimap: { enabled: false },
        automaticLayout: true,
        fontLigatures: true,
        smoothScrolling: true,
        fontFamily: `'Cascadia Code NF', 'Cascadia Code', Fira Code, JetBrains Mono, Roboto Mono, "Source Code Pro", Consolas, Menlo, Monaco, monospace, "Courier New", "Apple Color Emoji"${OS === 'windows' ? ', twemoji mozilla' : ''}`,
        quickSuggestions: {
          strings: true,
          comments: true,
          other: true,
        },
      }}
    />
  )
}
