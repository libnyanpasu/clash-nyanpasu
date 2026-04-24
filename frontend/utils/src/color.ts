export const alpha = (color: string, value: number) => {
  return `color-mix(in srgb, ${color} ${(value * 100).toFixed(2)}%, transparent ${((1 - value) * 100).toFixed(2)}%)`
}

export const lighten = (color: string, value: number) => {
  return `color-mix(in lch, ${color} ${((1 - value) * 100).toFixed(2)}%, white ${(value * 100).toFixed(2)}%)`
}

export const darken = (color: string, value: number) => {
  return `color-mix(in lch, ${color} ${((1 - value) * 100).toFixed(2)}%, black ${(value * 100).toFixed(2)}%)`
}
