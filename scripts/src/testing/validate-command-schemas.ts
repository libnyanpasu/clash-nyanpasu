import { readFileSync } from 'node:fs'
import path from 'node:path'
import type { AnySchemaObject, ErrorObject, ValidateFunction } from 'ajv'
import Ajv2020Module from 'ajv/dist/2020.js'
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts'

const Ajv2020 = Ajv2020Module as unknown as typeof Ajv2020Module.default

const DEFAULT_ARTIFACT_PATH = path.join(
  WORKSPACE_ROOT,
  'backend/nyanpasu-rpc-protocol/schemas/command-schemas.v1.json',
)

const DEFS_REF_PREFIX = '#/$defs/'

export interface CommandSchemaArtifact {
  version: number
  schemaDialect: string
  validationProfile?: string
  errorEnvelope: Record<string, unknown>
  defs: Record<string, Record<string, unknown>>
  commands: Record<
    string,
    {
      name: string
      effect: string
      scope: string
      targets: string[]
      input: Record<string, unknown>
      output: Record<string, unknown>
    }
  >
}

export interface BatchValidationCase {
  id: string
  schemaKey: string
  instance: unknown
  expectValid: boolean
}

export interface BatchValidationInput {
  schemas: Record<string, Record<string, unknown>>
  cases: BatchValidationCase[]
}

export interface BatchValidationFailure {
  id: string
  schemaKey: string
  expectValid: boolean
  actualValid: boolean
  reason: string
  errors?: ErrorObject[] | null
}

export interface BatchValidationResult {
  total: number
  passed: number
  failures: BatchValidationFailure[]
}

/**
 * Create a strict Draft 2020-12 Ajv validator that checks structure ONLY and never coerces,
 * defaults, or strips additional/privileged properties from caller payloads.
 */
export function createStrictValidator() {
  return new Ajv2020({
    strict: true,
    allErrors: true,
    coerceTypes: false,
    useDefaults: false,
    removeAdditional: false,
  })
}

const SINGLE_SUBSCHEMA_KEYWORDS = [
  'not',
  'if',
  'then',
  'else',
  'propertyNames',
  'contains',
  'additionalProperties',
  'unevaluatedProperties',
  'items',
  'unevaluatedItems',
  'contentSchema',
] as const

const ARRAY_SUBSCHEMA_KEYWORDS = [
  'allOf',
  'anyOf',
  'oneOf',
  'prefixItems',
] as const

const MAP_SUBSCHEMA_KEYWORDS = [
  'properties',
  'patternProperties',
  '$defs',
  'definitions',
  'dependentSchemas',
] as const

export function decodeDefsRefKey(reference: string): string {
  if (!reference.startsWith(DEFS_REF_PREFIX)) {
    throw new Error(`malformed JSON Schema $ref: ${reference}`)
  }
  const token = reference.slice(DEFS_REF_PREFIX.length)
  if (!token || token.includes('/')) {
    throw new Error(`malformed JSON Schema $ref: ${reference}`)
  }
  return token.replaceAll('~1', '/').replaceAll('~0', '~')
}

function collectSchemaRefs(node: unknown, out: string[]): void {
  if (typeof node !== 'object' || node === null || Array.isArray(node)) {
    return
  }
  const obj = node as Record<string, unknown>
  if ('$ref' in obj) {
    if (typeof obj.$ref !== 'string') {
      throw new Error(`malformed non-string $ref: ${JSON.stringify(obj.$ref)}`)
    }
    out.push(decodeDefsRefKey(obj.$ref))
  }
  for (const key of SINGLE_SUBSCHEMA_KEYWORDS) {
    if (key in obj) {
      collectSchemaRefs(obj[key], out)
    }
  }
  for (const key of ARRAY_SUBSCHEMA_KEYWORDS) {
    const val = obj[key]
    if (Array.isArray(val)) {
      for (const item of val) {
        collectSchemaRefs(item, out)
      }
    }
  }
  for (const key of MAP_SUBSCHEMA_KEYWORDS) {
    const val = obj[key]
    if (typeof val === 'object' && val !== null && !Array.isArray(val)) {
      for (const sub of Object.values(val)) {
        collectSchemaRefs(sub, out)
      }
    }
  }
}

export function resolveRootSchema(
  artifact: CommandSchemaArtifact,
  root: Record<string, unknown>,
  context: string,
): Record<string, unknown> {
  const queue: string[] = []
  collectSchemaRefs(root, queue)
  const seen = new Set<string>()
  const collectedDefs: Record<string, unknown> = {}

  while (queue.length > 0) {
    const defKey = queue.shift()!
    if (seen.has(defKey)) {
      continue
    }
    seen.add(defKey)
    const defSchema = artifact.defs[defKey]
    if (!defSchema) {
      throw new Error(`dangling $ref "${defKey}" while resolving ${context}`)
    }
    collectSchemaRefs(defSchema, queue)
    collectedDefs[defKey] = defSchema
  }

  const resolved: Record<string, unknown> = {
    $schema: artifact.schemaDialect,
  }
  if (!('$comment' in root) && artifact.validationProfile) {
    resolved.$comment = artifact.validationProfile
  }
  if (Object.keys(collectedDefs).length > 0) {
    resolved.$defs = collectedDefs
  }
  for (const [key, value] of Object.entries(root)) {
    if (key === '$schema' || key === '$defs') {
      continue
    }
    resolved[key] = value
  }
  return resolved
}

export function runBatchValidation(
  input: BatchValidationInput,
): BatchValidationResult {
  const ajv = createStrictValidator()
  const compiled = new Map<string, ValidateFunction>()

  for (const [key, schema] of Object.entries(input.schemas)) {
    compiled.set(key, ajv.compile(schema as AnySchemaObject))
  }

  const failures: BatchValidationFailure[] = []
  let passed = 0

  for (const testCase of input.cases) {
    const validate = compiled.get(testCase.schemaKey)
    if (!validate) {
      failures.push({
        id: testCase.id,
        schemaKey: testCase.schemaKey,
        expectValid: testCase.expectValid,
        actualValid: false,
        reason: `unknown schemaKey "${testCase.schemaKey}"`,
      })
      continue
    }

    const snapshotBefore = JSON.stringify(testCase.instance)
    const actualValid = validate(testCase.instance)
    const snapshotAfter = JSON.stringify(testCase.instance)

    if (snapshotBefore !== snapshotAfter) {
      failures.push({
        id: testCase.id,
        schemaKey: testCase.schemaKey,
        expectValid: testCase.expectValid,
        actualValid,
        reason: 'validator mutated input instance',
      })
      continue
    }

    if (actualValid !== testCase.expectValid) {
      failures.push({
        id: testCase.id,
        schemaKey: testCase.schemaKey,
        expectValid: testCase.expectValid,
        actualValid,
        reason: testCase.expectValid
          ? 'expected instance to be valid but Ajv rejected it'
          : 'expected instance to be invalid but Ajv accepted it',
        errors: validate.errors ?? null,
      })
      continue
    }

    passed += 1
  }

  return {
    total: input.cases.length,
    passed,
    failures,
  }
}

export function compileArtifactSchemas(artifactPath: string): {
  commandCount: number
  compiledSchemas: number
} {
  const raw = readFileSync(artifactPath, 'utf8')
  const artifact = JSON.parse(raw) as CommandSchemaArtifact
  const ajv = createStrictValidator()

  let compiledSchemas = 0
  ajv.compile(
    resolveRootSchema(artifact, artifact.errorEnvelope, 'errorEnvelope'),
  )
  compiledSchemas += 1

  const commandNames = Object.keys(artifact.commands)
  for (const name of commandNames) {
    const entry = artifact.commands[name]!
    ajv.compile(resolveRootSchema(artifact, entry.input, `${name}.input`))
    ajv.compile(resolveRootSchema(artifact, entry.output, `${name}.output`))
    compiledSchemas += 2
  }

  return {
    commandCount: commandNames.length,
    compiledSchemas,
  }
}

function main(): void {
  const args = process.argv.slice(2)
  if (args.includes('--batch')) {
    const raw = readFileSync(0, 'utf8')
    const input = JSON.parse(raw) as BatchValidationInput
    const result = runBatchValidation(input)
    process.stdout.write(`${JSON.stringify(result)}\n`)
    if (result.failures.length > 0) {
      process.exitCode = 1
    }
    return
  }

  let artifactPath = DEFAULT_ARTIFACT_PATH
  const fileIndex = args.indexOf('--file')
  if (fileIndex !== -1) {
    const supplied = args[fileIndex + 1]
    if (!supplied) {
      throw new Error(
        'Usage: validate-command-schemas.ts [--file <path> | --batch]',
      )
    }
    artifactPath = path.resolve(process.cwd(), supplied)
  }

  const summary = compileArtifactSchemas(artifactPath)
  process.stdout.write(
    `Ajv Draft 2020-12 strict compilation passed: ${summary.commandCount} commands (${summary.compiledSchemas} root schemas).\n`,
  )
}

if (import.meta.main) {
  main()
}
