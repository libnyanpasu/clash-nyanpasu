/* eslint-disable new-cap */
// features
// langs
import 'monaco-editor/esm/vs/basic-languages/javascript/javascript.contribution.js'
import 'monaco-editor/esm/vs/basic-languages/lua/lua.contribution.js'
import 'monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js'
import 'monaco-editor/esm/vs/editor/editor.all.js'
// language services
import * as monaco from 'monaco-editor/esm/vs/editor/editor.api'
import 'monaco-editor/esm/vs/language/typescript/monaco.contribution.js'
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker'
import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker'
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

export {}
