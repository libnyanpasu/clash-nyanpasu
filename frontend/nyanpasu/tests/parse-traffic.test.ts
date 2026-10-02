import { expect, test } from 'vitest'
import parseTraffic from '../src/utils/parse-traffic'

test('an amount just under the next unit never reads in exponent notation', () => {
  expect(parseTraffic(999.6 * 1024 * 1024)).toEqual(['1000', 'MiB'])
  expect(parseTraffic(999.4 * 1024 * 1024)).toEqual(['999', 'MiB'])
  expect(parseTraffic(1000 * 1024 * 1024)).toEqual(['1000', 'MiB'])
})
