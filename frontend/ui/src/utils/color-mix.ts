export const alpha = (color: string, alpha: number) => {
  return `color-mix(in srgb, ${color} ${(alpha * 100).toFixed(2)}%, transparent ${((1 - alpha) * 100).toFixed(2)}%)`
}

export const lighten = (color: string, alpha: number) => {
  return `color-mix(in lch, ${color} ${((1 - alpha) * 100).toFixed(2)}%, white ${(alpha * 100).toFixed(2)}%)`
}

export const darken = (color: string, alpha: number) => {
  return `color-mix(in lch, ${color} ${((1 - alpha) * 100).toFixed(2)}%, black ${(alpha * 100).toFixed(2)}%)`
}
