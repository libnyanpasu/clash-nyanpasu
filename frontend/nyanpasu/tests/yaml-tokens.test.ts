import { expect, test } from 'vitest'
import { lineTokens, type DocumentTokens } from '@/utils/yaml-tokens'

// Two lines: `key: value` as three tokens, then `- a` as one, with no color.
const document: DocumentTokens = {
  lineStarts: Uint32Array.of(0, 3, 4),
  tokens: Uint32Array.of(
    3,
    (1 << 4) | 1,
    2,
    0 << 4,
    5,
    (2 << 4) | 2,
    3,
    0 << 4,
  ),
  palette: ['', '#d08770', '#a3be8c'],
}

test('a line is cut into its tokens with their colors and font styles', () => {
  expect(lineTokens(document, 0, 'key: value')).toEqual([
    { text: 'key', color: '#d08770', fontStyle: 1 },
    { text: ': ', color: undefined, fontStyle: 0 },
    { text: 'value', color: '#a3be8c', fontStyle: 2 },
  ])
  expect(lineTokens(document, 1, '- a')).toEqual([
    { text: '- a', color: undefined, fontStyle: 0 },
  ])
})

test('text past the last token, a carriage return, is kept', () => {
  expect(lineTokens(document, 1, '- a\r').map(({ text }) => text)).toEqual([
    '- a',
    '\r',
  ])
})

test('a line the tokens do not cover is shown as it is', () => {
  expect(lineTokens(document, 2, 'extra')).toEqual([
    { text: 'extra', fontStyle: 0 },
  ])
})
