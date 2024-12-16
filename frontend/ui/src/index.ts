if (typeof window === 'undefined') {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  global.window = {} as any
}

export * from './chart'
export * from './hooks'
export * from './materialYou'
export * from './utils'
