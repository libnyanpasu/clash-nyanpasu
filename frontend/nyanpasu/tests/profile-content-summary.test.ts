import { expect, test } from 'vitest'
import { profileContentSummary } from '@/pages/(main)/main/profiles/$type/_modules/profile-content-summary'

test('counts static entries without expanding providers or group membership', () => {
  expect(
    profileContentSummary(`
proxies: [{ name: A }, { name: B }]
proxy-groups: [{ name: Auto, proxies: [A, B] }]
rules: [MATCH,DIRECT]
proxy-providers: { remote: { type: http } }
rule-providers: { remote: { type: http } }
`),
  ).toEqual({ nodes: 2, groups: 1, rules: 2 })
})

test('an empty mapping is zero; malformed documents and field shapes are unknown', () => {
  expect(profileContentSummary('{}')).toEqual({ nodes: 0, groups: 0, rules: 0 })
  for (const content of [
    'proxies: [',
    'proxies: [a]\nproxies: [b]',
    'proxies: invalid',
    '- item',
  ]) {
    expect(profileContentSummary(content)).toBeNull()
  }
})

test('counts YAML sequence aliases without expanding their contents', () => {
  expect(
    profileContentSummary('nodes: &nodes [{name: A}]\nproxies: *nodes'),
  ).toEqual({ nodes: 1, groups: 0, rules: 0 })
})
