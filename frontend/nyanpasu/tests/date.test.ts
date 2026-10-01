import { expect, test } from 'vitest'
import { formatDate, formatRelativeTime } from '../src/utils/date'

test('formats calendar dates, including dates around a week-year boundary', () => {
  const date = new Date(2021, 0, 1, 3, 4, 5)
  expect(formatDate(date)).toBe('2021-01-01 03:04:05')
  expect(formatDate(date, 'yyyy-MM-dd')).toBe('2021-01-01')
  expect(formatDate(date, 'HH:mm:ss')).toBe('03:04:05')
})

test('parses offset-free ISO dates in local time', () => {
  expect(formatDate('2024-02-29T03:04:05')).toBe('2024-02-29 03:04:05')
  expect(formatDate('2024-02-29')).toBe('2024-02-29 00:00:00')
})

test('formats UTC, explicit offsets, and millisecond timestamps as the same instant', () => {
  const instant = Date.UTC(2026, 0, 1, 0, 4, 5, 123)
  const expected = formatDate(new Date(instant))
  expect(formatDate(instant)).toBe(expected)
  expect(formatDate('2026-01-01T00:04:05.123456789Z')).toBe(expected)
  expect(formatDate('2026-01-01T08:04:05.123+08:00')).toBe(expected)
  expect(formatDate(0)).toBe(formatDate(new Date(0)))
})

test.each([
  ['en', '10 minutes ago', 'in 10 minutes'],
  ['ko', '10분 전', '10분 후'],
  ['ru', '10 минут назад', 'через 10 минут'],
  ['zh-cn', '10 分钟前', '10 分钟内'],
  ['zh-tw', '10 分鐘前', '10 分鐘內'],
] as const)('formats past and future times in %s', (language, past, future) => {
  const now = Date.UTC(2026, 0, 1, 12)
  expect(formatRelativeTime(now - 600_000, now, language)).toBe(past)
  expect(formatRelativeTime(now + 600_000, now, language)).toBe(future)
})

test('each call uses its supplied language without changing later calls', () => {
  const now = Date.UTC(2026, 0, 1, 12)
  const input = '2026-01-01T11:50:00Z'
  expect(formatRelativeTime(input, now, 'en')).toBe('10 minutes ago')
  expect(formatRelativeTime(input, now, 'zh-cn')).toBe('10 分钟前')
  expect(formatRelativeTime(input, now, 'en')).toBe('10 minutes ago')
})

test.each([null, undefined])(
  'renders missing provider timestamps as a placeholder',
  (input) => {
    expect(formatDate(input)).toBe('-')
    expect(formatRelativeTime(input, Date.UTC(2026, 0, 1), 'en')).toBe('-')
  },
)

test.each(['', 'not a date', '2026-02-30T12:00:00Z', NaN, new Date(NaN)])(
  'renders invalid input %s without throwing during a component render',
  (input) => {
    expect(formatDate(input)).toBe('Invalid Date')
    expect(formatRelativeTime(input, Date.UTC(2026, 0, 1), 'en')).toBe(
      'Invalid Date',
    )
  },
)
