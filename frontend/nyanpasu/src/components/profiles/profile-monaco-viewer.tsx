import { OS } from '@/consts'
import '@/services/monaco'
import { useAtomValue } from 'jotai'
import { type JSONSchema7 } from 'json-schema'
import nyanpasuMergeSchema from 'meta-json-schema/schemas/clash-nyanpasu-merge-json-schema.json'
import clashMetaSchema from 'meta-json-schema/schemas/meta-json-schema.json'
import { type editor } from 'monaco-editor'
import { configureMonacoYaml } from 'monaco-yaml'
import { nanoid } from 'nanoid'
import { useCallback, useMemo } from 'react'
// schema
import { themeMode } from '@/store'
import MonacoEditor, { type Monaco } from '@monaco-editor/react'
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

  const onChange = useCallback(
    (value: string | undefined) => {
      if (value && others.onChange) {
        others.onChange(value)
      }
    },
    [others],
  )

  return (
    <MonacoEditor
      className={cn(className)}
      value={value}
      language={language}
      path={path}
      theme={mode === 'light' ? 'vs' : 'vs-dark'}
      beforeMount={beforeEditorMount}
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
        fontFamily: `'Cascadia Code NF', 'Cascadia Code', Fira Code, JetBrains Mono, Roboto Mono, "Source Code Pro", Consolas, Menlo, Monaco, monospace, "Courier New", "Apple Color Emoji"${
          OS === 'windows' ? ', twemoji mozilla' : ''
        }`,
        quickSuggestions: {
          strings: true,
          comments: true,
          other: true,
        },
      }}
    />
  )
}
