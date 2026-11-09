import { afterEach, expect, test } from 'vitest'
import {
  getLocale,
  locales,
  overwriteGetLocale,
} from '../src/paraglide/runtime'
import { formatBytes } from '../src/utils/format-bytes'
import { byteUnits } from '../src/utils/information-unit'
import parseTraffic from '../src/utils/parse-traffic'

const latinByteUnits: readonly string[] = [
  'B',
  'KiB',
  'MiB',
  'GiB',
  'TiB',
  'PiB',
  'EiB',
  'ZiB',
  'YiB',
]

const russianByteUnits: readonly string[] = [
  'Б',
  'КиБ',
  'МиБ',
  'ГиБ',
  'ТиБ',
  'ПиБ',
  'ЭиБ',
  'ЗиБ',
  'ЙиБ',
]

const previousGetLocale = getLocale

afterEach(() => overwriteGetLocale(previousGetLocale))

test('the byte-unit list matches the IEC exponents', () => {
  expect(byteUnits).toEqual(latinByteUnits)
})

test.each(locales)('IEC byte units follow the %s locale', (locale) => {
  overwriteGetLocale(() => locale)
  const labels = locale === 'ru' ? russianByteUnits : latinByteUnits

  labels.forEach((unit, exponent) => {
    expect(formatBytes(1024 ** exponent)).toBe(`1 ${unit}`)
  })
})

test('Russian renders Cyrillic SI bit units', () => {
  overwriteGetLocale(() => 'ru')

  expect(formatBytes(0)).toBe('0 Б')
  expect(formatBytes(1, { standard: 'si', bits: true })).toBe('8 бит')
  expect(formatBytes(125, { standard: 'si', bits: true })).toBe('1 кбит')
  expect(formatBytes(125_000, { standard: 'si', bits: true })).toBe('1 Мбит')
})

test.each(locales.filter((locale) => locale !== 'ru'))(
  'other locales keep IEC symbols in %s',
  (locale) => {
    overwriteGetLocale(() => locale)

    expect(formatBytes(1024)).toBe('1 KiB')
    expect(formatBytes(125, { standard: 'si', bits: true })).toBe('1 kbit')
  },
)

test('parseTraffic localizes its unit without changing the amount', () => {
  const cases = [
    ['en', ['1.00', 'KiB']],
    ['ja', ['1.00', 'KiB']],
    ['ko', ['1.00', 'KiB']],
    ['ru', ['1.00', 'КиБ']],
    ['zh-cn', ['1.00', 'KiB']],
    ['zh-tw', ['1.00', 'KiB']],
  ] as const

  for (const [locale, expected] of cases) {
    overwriteGetLocale(() => locale)
    expect(parseTraffic(1024)).toEqual(expected)
  }

  overwriteGetLocale(() => 'ru')
  expect(parseTraffic(0)).toEqual(['0', 'Б'])
})
