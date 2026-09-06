/* eslint-disable new-cap */
// features
// langs
import 'monaco-editor/languages/definitions/javascript/register'
import 'monaco-editor/languages/definitions/lua/register'
import 'monaco-editor/languages/definitions/yaml/register'
import 'monaco-editor/features/register.all'
import 'monaco-editor/features/links/register'
// language services
import * as monaco from 'monaco-editor'
import 'monaco-editor/languages/features/typescript/register'
import editorWorker from 'monaco-editor/editor/editor.worker?worker'
import cssWorker from 'monaco-editor/language/css/css.worker?worker'
import jsonWorker from 'monaco-editor/language/json/json.worker?worker'
import tsWorker from 'monaco-editor/language/typescript/ts.worker?worker'
// workers
import yamlWorker from '@/utils/monaco-yaml.worker?worker'
// others
import { loader } from '@monaco-editor/react'

self.MonacoEnvironment = {
  getWorker(_, label) {
    switch (label) {
      case 'json':
        return new jsonWorker()
      case 'typescript':
      case 'javascript':
        return new tsWorker()
      case 'css':
      case 'less':
      case 'scss':
        return new cssWorker()
      case 'yaml':
        return new yamlWorker()
      default:
        return new editorWorker()
    }
  },
}

loader.config({ monaco })

loader
  .init()
  .then(() => {
    console.log('Monaco is ready')
  })
  .catch((error) => {
    console.error('Monaco initialization failed', error)
  })
