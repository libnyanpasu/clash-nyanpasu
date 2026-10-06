export type Breakpoint = 'xs' | 'sm' | 'md' | 'lg' | 'xl'

export const BREAKPOINT_VALUES = {
  xs: 0,
  sm: 600,
  md: 900,
  lg: 1200,
  xl: 1536,
} as const satisfies Record<Breakpoint, number>

export const BREAKPOINT_ORDER = ['xs', 'sm', 'md', 'lg', 'xl'] as const

/** The URL latency tests use when no default test URL is configured. */
export const DEFAULT_LATENCY_TEST_URL = 'http://www.gstatic.com/generate_204'
