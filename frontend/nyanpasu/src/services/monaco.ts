import nyanpasuMergeSchema from "meta-json-schema/schemas/clash-nyanpasu-merge-json-schema.json";
import clashMetaSchema from "meta-json-schema/schemas/meta-json-schema.json";
import { configureMonacoYaml } from "monaco-yaml";
// features
// langs
import "monaco-editor/esm/vs/basic-languages/javascript/javascript.contribution.js";
import "monaco-editor/esm/vs/basic-languages/lua/lua.contribution.js";
import "monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js";
import "monaco-editor/esm/vs/editor/editor.all.js";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api";
// language services
import "monaco-editor/esm/vs/language/typescript/monaco.contribution.js";

monaco.languages.typescript.javascriptDefaults.setCompilerOptions({
  target: monaco.languages.typescript.ScriptTarget.ES2020,
  allowNonTsExtensions: true,
  allowJs: true,
});

configureMonacoYaml(monaco, {
  validate: true,
  enableSchemaRequest: true,
  schemas: [
    {
      fileMatch: ["**/*.clash.yaml"],
      // @ts-expect-error monaco-yaml parse issue
      schema: clashMetaSchema,
    },
    {
      fileMatch: ["**/*.merge.yaml"],
      // @ts-expect-error monaco-yaml parse issue
      schema: nyanpasuMergeSchema,
    },
  ],
});

export { monaco };
