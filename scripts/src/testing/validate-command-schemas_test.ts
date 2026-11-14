import assert from 'node:assert/strict'
import path from 'node:path'
import { test } from 'node:test'
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts'
import {
  compileArtifactSchemas,
  decodeDefsRefKey,
  resolveRootSchema,
  runBatchValidation,
  type CommandSchemaArtifact,
} from './validate-command-schemas.ts'

test('declared Ajv2020 compiles the entire embedded command schema catalog in strict mode', () => {
  const artifactPath = path.join(
    WORKSPACE_ROOT,
    'backend/nyanpasu-rpc-protocol/schemas/command-schemas.v1.json',
  )
  const summary = compileArtifactSchemas(artifactPath)
  assert.ok(summary.commandCount >= 100)
  assert.equal(summary.compiledSchemas, summary.commandCount * 2 + 1)
})

test('validator runs strict Draft 2020-12 without coercing, defaulting, or mutating caller instances', () => {
  const payload = {
    keep: 1,
    privilegedOrUnknown: true,
    $ref: 'a',
  }
  const originalJson = JSON.stringify(payload)

  const result = runBatchValidation({
    schemas: {
      open_patch: {
        $schema: 'https://json-schema.org/draft/2020-12/schema',
        type: 'object',
        properties: {
          keep: { type: 'integer', default: 99 },
          count: { type: 'integer', minimum: 0 },
          $ref: { type: 'string', minLength: 1, maxLength: 1 },
        },
        additionalProperties: true,
      },
    },
    cases: [
      {
        id: 'preserves-open-unknown-and-skips-defaults',
        schemaKey: 'open_patch',
        instance: payload,
        expectValid: true,
      },
      {
        id: 'rejects-string-number-without-coercion',
        schemaKey: 'open_patch',
        instance: { keep: '1' },
        expectValid: false,
      },
      {
        id: 'rejects-multi-char-string-on-char-field',
        schemaKey: 'open_patch',
        instance: { $ref: 'ab' },
        expectValid: false,
      },
    ],
  })

  assert.equal(result.total, 3)
  assert.equal(result.passed, 3)
  assert.deepEqual(result.failures, [])
  assert.equal(JSON.stringify(payload), originalJson)
})

test('resolver handles RFC 6901 escapes and ignores literal $ref inside annotations', () => {
  assert.equal(decodeDefsRefKey('#/$defs/a~1b~0c'), 'a/b~c')

  const artifact: CommandSchemaArtifact = {
    version: 1,
    schemaDialect: 'https://json-schema.org/draft/2020-12/schema',
    errorEnvelope: { type: 'object' },
    defs: {
      'a/b~c': { type: 'string', minLength: 1 },
      unused: { type: 'boolean' },
    },
    commands: {},
  }

  const resolved = resolveRootSchema(
    artifact,
    {
      type: 'object',
      properties: {
        $ref: { $ref: '#/$defs/a~1b~0c' },
      },
      examples: [{ $ref: 'not-a-pointer' }],
      required: ['$ref'],
    },
    'probe.input',
  )

  const defs = resolved.$defs as Record<string, unknown>
  assert.deepEqual(Object.keys(defs), ['a/b~c'])
})
