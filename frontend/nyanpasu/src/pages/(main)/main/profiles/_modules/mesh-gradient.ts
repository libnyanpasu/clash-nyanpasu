/** An RGB color with channels from 0 to 1. */
export type MeshGradientColor = readonly [number, number, number]

export type MeshGradientOptions = {
  colors: readonly MeshGradientColor[]
  distortion: number
  swirl: number
}

const smoothstep = (x: number) => {
  const t = Math.min(1, Math.max(0, x))
  return t * t * (3 - 2 * t)
}

/**
 * Draws `@paper-design/shaders`' mesh gradient, as `MeshGradient` renders it
 * with no grain and its default sizing (`fit: contain`), into an RGBA buffer
 * of `width` by `height` at `time` seconds of its clock. It is the fragment
 * shader evaluated per pixel on the CPU: a smooth gradient that a small buffer
 * shows as well when scaled up, without a WebGL context.
 */
export function drawMeshGradient(
  pixels: Uint8ClampedArray,
  width: number,
  height: number,
  time: number,
  { colors, distortion, swirl }: MeshGradientOptions,
) {
  const t = 0.5 * (time + 41.5)

  // Where each color sits at this time.
  const xs = colors.map((_, i) => {
    const b = 0.6 + ((i / 3) % 1) * 0.9
    return 0.5 + 0.5 * Math.sin(t * b + i * 0.37)
  })
  const ys = colors.map((_, i) => {
    const c = 0.8 + (((i + 1) / 4) % 1)
    return 0.5 + 0.5 * Math.cos(t * c + i * 0.37 * 1.5)
  })

  // `contain` fits a unit square to the shorter side; the longer one extends
  // past [0, 1]. Rows run top down, the shader's y up.
  const side = Math.min(width, height)
  let index = 0

  for (let row = 0; row < height; row++) {
    const y0 = (0.5 - (row + 0.5) / height) * (height / side) + 0.5

    for (let column = 0; column < width; column++, index += 4) {
      let x = ((column + 0.5) / width - 0.5) * (width / side) + 0.5
      let y = y0

      const radius = smoothstep(Math.hypot(x - 0.5, y - 0.5))
      const center = 1 - radius
      for (let i = 1; i <= 2; i++) {
        x +=
          ((distortion * center) / i) *
          Math.sin(t + i * 0.4 * smoothstep(y)) *
          Math.cos(0.2 * t + i * 2.4 * smoothstep(y))
        y += ((distortion * center) / i) * Math.cos(t + i * 2 * smoothstep(x))
      }

      const angle = -3 * swirl * radius
      const cos = Math.cos(angle)
      const sin = Math.sin(angle)
      const rx = x - 0.5
      const ry = y - 0.5
      const qx = cos * rx - sin * ry + 0.5
      const qy = sin * rx + cos * ry + 0.5

      let r = 0
      let g = 0
      let b = 0
      let total = 0
      for (let i = 0; i < colors.length; i++) {
        const dx = qx - xs[i]
        const dy = qy - ys[i]
        const weight = 1 / ((dx * dx + dy * dy) ** 1.75 + 1e-3)
        r += colors[i][0] * weight
        g += colors[i][1] * weight
        b += colors[i][2] * weight
        total += weight
      }

      pixels[index] = (r / total) * 255
      pixels[index + 1] = (g / total) * 255
      pixels[index + 2] = (b / total) * 255
      pixels[index + 3] = 255
    }
  }
}
