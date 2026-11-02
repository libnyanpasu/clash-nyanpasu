import assert from 'node:assert/strict'
import { test } from 'node:test'
import { containBackgroundIcon } from './finalize-macos-dmg.ts'

test('DMG background relocation preserves the rest of the Finder metadata', () => {
  // Iloc record captured from create-dmg, including its 16-byte location blob.
  const hex =
    '0000000b002e006200610063006b00670072006f0075006e0064' +
    '496c6f63626c6f6200000010000003b200000064ffffffffffff0000'
  const record = Uint8Array.from(hex.match(/../g)!, (pair) =>
    parseInt(pair, 16),
  )
  const original = new Uint8Array([1, 2, 3, ...record, 4, 5, 6])
  const result = containBackgroundIcon(original)
  const expected = original.slice()
  const position = 3 + record.length - 16
  const view = new DataView(expected.buffer)
  view.setUint32(position, 160)
  view.setUint32(position + 4, 100)

  assert.deepStrictEqual(result, expected)
  assert.deepStrictEqual(original.slice(3, -3), record)
  assert.deepStrictEqual(containBackgroundIcon(result), result)
})

test('DMG finalization refuses missing or truncated location metadata', () => {
  assert.throws(() => containBackgroundIcon(new Uint8Array()), {
    message: /Missing/,
  })
  assert.throws(
    () =>
      containBackgroundIcon(new TextEncoder().encode('.backgroundIlocblob')),
    { message: /Missing/ },
  )
})
