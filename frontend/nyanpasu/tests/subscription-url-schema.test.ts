import { expect, test } from 'vitest'
import { subscriptionUrlSchema } from '../src/pages/(main)/main/profiles/_modules/subscription-url-schema'

test.each([
  'https://192.0.2.1:8100/api/v1/client/subscribe?token=xxx&clash=1',
  'https://[2001:db8::1]:8100/api/v1/client/subscribe?token=xxx&clash=1',
  'http://subscription.example.com:8100/api/v1/client/subscribe?token=xxx&clash=1',
  'https://subscription.example.com/api/v1/client/subscribe?token=xxx&clash=1',
])('accepts HTTP(S) subscription URL %s', (url) => {
  expect(subscriptionUrlSchema.safeParse(url).success).toBe(true)
})

test.each([
  'https://114.514.19.19:8100/api/v1/client/subscribe?token=xxx&clash=1',
  'https://',
  'ftp://subscription.example.com/subscribe',
  'subscription.example.com/subscribe',
])('rejects malformed or unsupported subscription URL %s', (url) => {
  expect(subscriptionUrlSchema.safeParse(url).success).toBe(false)
})
