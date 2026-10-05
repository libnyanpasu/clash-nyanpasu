import { expect, test } from 'vitest'
import {
  fingerprint,
  firstFrame,
  normalizeMessage,
} from '@/services/error-reporting/fingerprint'
import {
  describeError,
  fromConsole,
  fromReact,
  fromRejection,
  serializeValue,
  stripQuery,
} from '@/services/error-reporting/normalize'

test('serializes values without throwing on cycles, depth or size', () => {
  const cyclic: Record<string, unknown> = { name: 'node' }
  cyclic.self = cyclic
  expect(serializeValue(cyclic)).toBe('{"name":"node","self":"[Circular]"}')

  expect(serializeValue({ a: { b: { c: { d: { e: 1 } } } } })).toBe(
    '{"a":{"b":{"c":{"d":"[Object]"}}}}',
  )
  expect(serializeValue(10n)).toBe('10n')
  expect(serializeValue(undefined)).toBe('[undefined]')
  expect(serializeValue(function load() {})).toBe('[Function load]')
  expect(serializeValue(new TypeError('bad'))).toBe('TypeError: bad')
  expect(serializeValue('x'.repeat(5000))).toHaveLength(4096)

  const hostile = {
    get value() {
      throw new Error('getter')
    },
  }
  expect(serializeValue(hostile)).toBe('[object Object]')
})

test('describes an error with its bounded cause chain', () => {
  const root = new Error('root')
  const middle = new Error('middle', { cause: root })
  const error = new RangeError('outer', { cause: middle })

  const described = describeError(error)

  expect(described.message).toBe('outer')
  expect(described.error_name).toBe('RangeError')
  expect(described.stack).toContain('outer')
  expect(described.causes.map((cause) => cause.message)).toEqual([
    'middle',
    'root',
  ])

  const looped = new Error('a')
  looped.cause = looped
  expect(describeError(looped).causes).toEqual([])

  const deep = Array.from({ length: 6 }).reduce<Error>(
    (cause, _, i) => new Error(`level ${i}`, { cause }),
    new Error('bottom'),
  )
  expect(describeError(deep).causes).toHaveLength(3)

  expect(describeError({ code: 1 })).toEqual({
    message: '{"code":1}',
    error_name: null,
    stack: null,
    causes: [],
  })
})

test('console arguments become one message and keep the first error stack', () => {
  const error = new Error('broken')
  const draft = fromConsole('error', ['failed to load', error, { id: 1 }])

  expect(draft).toMatchObject({
    kind: 'console',
    level: 'error',
    message: 'failed to load Error: broken {"id":1}',
    error_name: 'Error',
    stack: error.stack,
  })
  expect(fromConsole('warning', ['plain']).stack).toBeNull()
})

test('rejections and React errors map to their kinds and levels', () => {
  expect(fromRejection('nope')).toMatchObject({
    kind: 'unhandled_rejection',
    level: 'error',
    message: 'nope',
  })
  expect(
    fromReact('react_caught', new Error('x'), '\n    at Page'),
  ).toMatchObject({
    kind: 'react_caught',
    level: 'error',
    component_stack: '\n    at Page',
  })
  expect(
    fromReact('react_recoverable', new Error('x'), undefined),
  ).toMatchObject({ level: 'warning', component_stack: null })
})

test('query strings are dropped from URLs and frames', () => {
  expect(stripQuery('https://host/sub?token=secret#frag')).toBe(
    'https://host/sub',
  )
  expect(
    firstFrame(
      'Error: x\n    at load (http://localhost/assets/app.js?v=3:10:5)\n    at run',
    ),
  ).toBe('at load (http://localhost/assets/app.js:10:5)')
  expect(firstFrame('load@http://localhost/app.js:1:2\nrun@x')).toBe(
    'load@http://localhost/app.js:1:2',
  )
  expect(firstFrame(null)).toBe('')
})

test('fingerprints group repeats that differ only in variable values', () => {
  const draft = (message: string) =>
    fromConsole('error', [message, new Error('same')])

  expect(
    normalizeMessage(
      'id 42 at 7f3a9c0b1d2e uuid 0b0f1c3e-1111-4a2b-8c3d-1234567890ab',
    ),
  ).toBe('id <n> at <hex> uuid <uuid>')
  expect(fingerprint(draft('retry 1 of 3'))).toBe(
    fingerprint(draft('retry 2 of 3')),
  )
  expect(fingerprint(draft('retry 1 of 3'))).not.toBe(
    fingerprint(draft('timeout')),
  )
  expect(fingerprint(draft('x'))).toMatch(/^console-[0-9a-f]+$/)
  expect(fingerprint(fromRejection('x'))).toMatch(/^unhandled_rejection-/)
})
