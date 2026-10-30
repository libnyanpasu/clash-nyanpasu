import { describe, expect, it } from 'vitest'
import {
  cycleIndex,
  diffLines,
  fieldLocator,
  findLines,
} from '@/pages/(main)/main/profiles/inspect/_modules/text-find'

describe('findLines', () => {
  const lines = [
    'port: 7890',
    'dns:',
    '  Nameserver:',
    '    - 1.1.1.1',
    'rules: []',
  ]

  it('returns the indexes of matching lines, ignoring case', () => {
    expect(findLines(lines, 'nameserver')).toEqual([2])
    expect(findLines(lines, 'S')).toEqual([1, 2, 4])
  })

  it('finds nothing for an empty query', () => {
    expect(findLines(lines, '')).toEqual([])
    expect(findLines([], 'a')).toEqual([])
  })
})

describe('cycleIndex', () => {
  it('wraps at both ends', () => {
    expect(cycleIndex(2, 3, 1)).toBe(0)
    expect(cycleIndex(0, 3, -1)).toBe(2)
    expect(cycleIndex(1, 3, 1)).toBe(2)
  })

  it('stays at zero without matches', () => {
    expect(cycleIndex(0, 0, 1)).toBe(0)
  })
})

describe('diffLines', () => {
  it('drops the sign and keeps the order across hunks', () => {
    expect(
      diffLines([
        {
          old_start: 1,
          old_lines: 2,
          new_start: 1,
          new_lines: 2,
          lines: [' a: 1', '-b: 2'],
        },
        {
          old_start: 9,
          old_lines: 1,
          new_start: 9,
          new_lines: 1,
          lines: ['+c: 3'],
        },
      ]),
    ).toEqual(['a: 1', 'b: 2', 'c: 3'])
  })
})

describe('fieldLocator', () => {
  const yaml = [
    '# config',
    'port: 7890',
    'dns:',
    '  enable: true',
    '  nameserver:',
    '    - 1.1.1.1',
    '    - 8.8.8.8',
    '  fallback: []',
    'rules:',
    '- DOMAIN,a,DIRECT',
    '- MATCH,DIRECT',
    '"quoted": 1',
    'proxies: []',
  ]
  const locate = fieldLocator(yaml)

  it('finds a top-level key and the end of its block', () => {
    expect(locate('port')).toEqual({ start: 1, end: 1 })
    expect(locate('dns')).toEqual({ start: 2, end: 7 })
  })

  it('counts a sequence at the key indent as part of the block', () => {
    expect(locate('rules')).toEqual({ start: 8, end: 10 })
  })

  it('walks a dotted path through the nesting', () => {
    expect(locate('dns.nameserver')).toEqual({
      start: 4,
      end: 6,
    })
    expect(locate('dns.fallback')).toEqual({ start: 7, end: 7 })
  })

  it('finds a quoted key', () => {
    expect(locate('quoted')).toEqual({ start: 11, end: 11 })
  })

  it('does not match a nested key at the top level or a longer key', () => {
    expect(locate('enable')).toBeNull()
    expect(locate('prox')).toBeNull()
    expect(locate('dns.missing')).toBeNull()
  })

  it('treats regular expression characters in a key literally', () => {
    expect(fieldLocator(['a.b: 1', 'axb: 2'])('a.b')).toBeNull()
    expect(fieldLocator(['a+b: 1'])('a+b')).toEqual({ start: 0, end: 0 })
  })
})
