import { format, formatDistance, isValid, parseISO } from 'date-fns'
import { enUS, ko, ru, zhCN, zhTW } from 'date-fns/locale'
import type { Locale } from '@/paraglide/runtime'

const DATE_LOCALES = {
  en: enUS,
  ko,
  ru,
  'zh-cn': zhCN,
  'zh-tw': zhTW,
} satisfies Record<Locale, typeof enUS>

type DateInput = Date | number | string | null | undefined

const toDate = (value: NonNullable<DateInput>) =>
  typeof value === 'string' ? parseISO(value) : new Date(value)

export const formatDate = (
  value: DateInput,
  pattern = 'yyyy-MM-dd HH:mm:ss',
): string => {
  if (value == null) {
    return '-'
  }
  const date = toDate(value)
  return isValid(date) ? format(date, pattern) : 'Invalid Date'
}

export const formatRelativeTime = (
  value: DateInput,
  now: Date | number,
  language: Locale,
): string => {
  if (value == null) {
    return '-'
  }
  const date = toDate(value)
  return isValid(date)
    ? formatDistance(date, now, {
        addSuffix: true,
        locale: DATE_LOCALES[language],
      })
    : 'Invalid Date'
}
