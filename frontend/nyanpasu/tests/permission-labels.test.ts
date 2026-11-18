import { readFileSync } from 'node:fs'
import path from 'node:path'
import { expect, test } from 'vitest'
import {
  ACCESS_LABELS,
  RESOURCE_LABELS,
  ROLE_LABELS,
} from '../src/pages/(main)/main/settings/nyanpasu/remote-access/_modules/permission-labels'

const LOCALES = ['en', 'ja', 'ko', 'ru', 'zh-cn', 'zh-tw'] as const

function messages(locale: (typeof LOCALES)[number]): Record<string, unknown> {
  return JSON.parse(
    readFileSync(
      path.resolve('frontend/nyanpasu/messages', `${locale}.json`),
      'utf8',
    ),
  )
}

// The label maps are typed against the generated `Resource`, `Access` and `Role`
// unions, so a new resource, level or role fails to typecheck until it has a
// label; this checks that every locale translates each of them.
const KEYS = [
  ...Object.keys(RESOURCE_LABELS).map(
    (resource) => `settings_remote_access_resource_${resource}`,
  ),
  ...Object.keys(ACCESS_LABELS).map(
    (access) => `settings_remote_access_access_${access}`,
  ),
  'settings_remote_access_access_none',
  ...Object.keys(ROLE_LABELS).map(
    (role) => `settings_remote_access_role_${role}`,
  ),
]

test.each(LOCALES)(
  'every resource, level and role has a %s label',
  (locale) => {
    const translated = messages(locale)
    for (const key of KEYS) {
      expect(translated[key], key).toEqual(expect.any(String))
      expect((translated[key] as string).trim(), key).not.toBe('')
    }
  },
)

test('the labels cover the eight resources, both levels and five roles', () => {
  expect(Object.keys(RESOURCE_LABELS)).toHaveLength(8)
  expect(Object.keys(ACCESS_LABELS)).toEqual(['read', 'write'])
  expect(Object.keys(ROLE_LABELS)).toHaveLength(5)
})
