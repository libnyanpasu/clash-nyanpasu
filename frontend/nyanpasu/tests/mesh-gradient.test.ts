import { expect, test } from 'vitest'
import {
  drawMeshGradient,
  type MeshGradientColor,
} from '@/pages/(main)/main/profiles/_modules/mesh-gradient'

const draw = (colors: MeshGradientColor[], width = 12, height = 4) => {
  const pixels = new Uint8ClampedArray(width * height * 4)
  drawMeshGradient(pixels, width, height, 0, {
    colors,
    distortion: 0.5,
    swirl: 0.1,
  })
  return pixels
}

const pixelsOf = (data: Uint8ClampedArray) =>
  Array.from({ length: data.length / 4 }, (_, i) => [
    ...data.subarray(i * 4, i * 4 + 4),
  ])

test('one color fills every pixel, opaque', () => {
  for (const pixel of pixelsOf(draw([[0.2, 0.4, 0.6]]))) {
    expect(pixel).toEqual([51, 102, 153, 255])
  }
})

test('every pixel mixes the colors, in a buffer wider than tall', () => {
  const pixels = pixelsOf(
    draw([
      [1, 0, 0],
      [0, 0, 1],
    ]),
  )

  for (const [r, g, b, a] of pixels) {
    expect(g).toBe(0)
    expect(r + b).toBeGreaterThanOrEqual(254)
    expect(r + b).toBeLessThanOrEqual(256)
    expect(a).toBe(255)
  }
  // Both colors show somewhere.
  expect(Math.max(...pixels.map(([r]) => r))).toBeGreaterThan(128)
  expect(Math.max(...pixels.map(([, , b]) => b))).toBeGreaterThan(128)
})
