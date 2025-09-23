import {
  argbFromHex,
  hexFromArgb,
  themeFromSourceColor,
} from '@material/material-color-utilities'

/**
 * Material Design 3 Design Token System
 * Generates CSS custom properties from Material You color schemes
 */

export interface MD3ColorScheme {
  // Primary colors
  primary: string
  onPrimary: string
  primaryContainer: string
  onPrimaryContainer: string

  // Secondary colors
  secondary: string
  onSecondary: string
  secondaryContainer: string
  onSecondaryContainer: string

  // Tertiary colors
  tertiary: string
  onTertiary: string
  tertiaryContainer: string
  onTertiaryContainer: string

  // Error colors
  error: string
  onError: string
  errorContainer: string
  onErrorContainer: string

  // Surface colors
  surface: string
  onSurface: string
  surfaceVariant: string
  onSurfaceVariant: string
  surfaceDim: string
  surfaceBright: string
  surfaceContainerLowest: string
  surfaceContainerLow: string
  surfaceContainer: string
  surfaceContainerHigh: string
  surfaceContainerHighest: string

  // Background colors
  background: string
  onBackground: string

  // Outline colors
  outline: string
  outlineVariant: string

  // Other colors
  shadow: string
  scrim: string
  inverseSurface: string
  inverseOnSurface: string
  inversePrimary: string
}

export interface MD3Tokens {
  colors: {
    light: MD3ColorScheme
    dark: MD3ColorScheme
  }
  typography: {
    fontFamily: string
  }
  spacing: Record<string, string>
  borderRadius: Record<string, string>
  elevation: Record<string, string>
  breakpoints: Record<string, number>
}

/**
 * Generate MD3 color scheme from Material Color Utilities
 */
export const generateMD3ColorScheme = (
  materialScheme: any,
  mode: 'light' | 'dark',
): MD3ColorScheme => {
  const scheme = materialScheme.schemes[mode]

  return {
    // Primary colors
    primary: hexFromArgb(scheme.primary),
    onPrimary: hexFromArgb(scheme.onPrimary),
    primaryContainer: hexFromArgb(scheme.primaryContainer),
    onPrimaryContainer: hexFromArgb(scheme.onPrimaryContainer),

    // Secondary colors
    secondary: hexFromArgb(scheme.secondary),
    onSecondary: hexFromArgb(scheme.onSecondary),
    secondaryContainer: hexFromArgb(scheme.secondaryContainer),
    onSecondaryContainer: hexFromArgb(scheme.onSecondaryContainer),

    // Tertiary colors
    tertiary: hexFromArgb(scheme.tertiary),
    onTertiary: hexFromArgb(scheme.onTertiary),
    tertiaryContainer: hexFromArgb(scheme.tertiaryContainer),
    onTertiaryContainer: hexFromArgb(scheme.onTertiaryContainer),

    // Error colors
    error: hexFromArgb(scheme.error),
    onError: hexFromArgb(scheme.onError),
    errorContainer: hexFromArgb(scheme.errorContainer),
    onErrorContainer: hexFromArgb(scheme.onErrorContainer),

    // Surface colors
    surface: hexFromArgb(scheme.surface),
    onSurface: hexFromArgb(scheme.onSurface),
    surfaceVariant: hexFromArgb(scheme.surfaceVariant),
    onSurfaceVariant: hexFromArgb(scheme.onSurfaceVariant),
    surfaceDim: hexFromArgb(scheme.surfaceDim),
    surfaceBright: hexFromArgb(scheme.surfaceBright),
    surfaceContainerLowest: hexFromArgb(scheme.surfaceContainerLowest),
    surfaceContainerLow: hexFromArgb(scheme.surfaceContainerLow),
    surfaceContainer: hexFromArgb(scheme.surfaceContainer),
    surfaceContainerHigh: hexFromArgb(scheme.surfaceContainerHigh),
    surfaceContainerHighest: hexFromArgb(scheme.surfaceContainerHighest),

    // Background colors
    background: hexFromArgb(scheme.background),
    onBackground: hexFromArgb(scheme.onBackground),

    // Outline colors
    outline: hexFromArgb(scheme.outline),
    outlineVariant: hexFromArgb(scheme.outlineVariant),

    // Other colors
    shadow: hexFromArgb(scheme.shadow),
    scrim: hexFromArgb(scheme.scrim),
    inverseSurface: hexFromArgb(scheme.inverseSurface),
    inverseOnSurface: hexFromArgb(scheme.inverseOnSurface),
    inversePrimary: hexFromArgb(scheme.inversePrimary),
  }
}

/**
 * Create MD3 design tokens from source color
 */
export const createMD3Tokens = (
  sourceColor: string,
  fontFamily: string = 'system-ui, -apple-system, sans-serif',
): MD3Tokens => {
  const materialColor = themeFromSourceColor(argbFromHex(sourceColor))

  return {
    colors: {
      light: generateMD3ColorScheme(materialColor, 'light'),
      dark: generateMD3ColorScheme(materialColor, 'dark'),
    },
    typography: {
      fontFamily,
    },
    spacing: {
      '0': '0px',
      '1': '4px',
      '2': '8px',
      '3': '12px',
      '4': '16px',
      '5': '20px',
      '6': '24px',
      '8': '32px',
      '10': '40px',
      '12': '48px',
      '16': '64px',
      '20': '80px',
      '24': '96px',
      '32': '128px',
    },
    borderRadius: {
      none: '0px',
      xs: '4px',
      sm: '8px',
      md: '12px',
      lg: '16px',
      xl: '24px',
      '2xl': '32px',
      '3xl': '48px',
      full: '9999px',
    },
    elevation: {
      '0': '0px 0px 0px 0px rgba(0, 0, 0, 0)',
      '1': '0px 1px 2px 0px rgba(0, 0, 0, 0.3), 0px 1px 3px 1px rgba(0, 0, 0, 0.15)',
      '2': '0px 1px 2px 0px rgba(0, 0, 0, 0.3), 0px 2px 6px 2px rgba(0, 0, 0, 0.15)',
      '3': '0px 4px 8px 3px rgba(0, 0, 0, 0.15), 0px 1px 3px 0px rgba(0, 0, 0, 0.3)',
      '4': '0px 6px 10px 4px rgba(0, 0, 0, 0.15), 0px 2px 3px 0px rgba(0, 0, 0, 0.3)',
      '5': '0px 8px 12px 6px rgba(0, 0, 0, 0.15), 0px 4px 4px 0px rgba(0, 0, 0, 0.3)',
    },
    breakpoints: {
      xs: 0,
      sm: 400,
      md: 800,
      lg: 1200,
      xl: 1600,
    },
  }
}

/**
 * Convert MD3 tokens to CSS custom properties
 */
export const generateCSSCustomProperties = (tokens: MD3Tokens) => {
  const lightProperties: Record<string, string> = {}
  const darkProperties: Record<string, string> = {}

  // Color properties
  Object.entries(tokens.colors.light).forEach(([key, value]) => {
    const cssVar = `--md3-color-${key.replace(/([A-Z])/g, '-$1').toLowerCase()}`
    lightProperties[cssVar] = value
  })

  Object.entries(tokens.colors.dark).forEach(([key, value]) => {
    const cssVar = `--md3-color-${key.replace(/([A-Z])/g, '-$1').toLowerCase()}`
    darkProperties[cssVar] = value
  })

  // Typography properties
  lightProperties['--md3-font-family'] = tokens.typography.fontFamily
  darkProperties['--md3-font-family'] = tokens.typography.fontFamily

  // Spacing properties
  Object.entries(tokens.spacing).forEach(([key, value]) => {
    lightProperties[`--md3-spacing-${key}`] = value
    darkProperties[`--md3-spacing-${key}`] = value
  })

  // Border radius properties
  Object.entries(tokens.borderRadius).forEach(([key, value]) => {
    lightProperties[`--md3-radius-${key}`] = value
    darkProperties[`--md3-radius-${key}`] = value
  })

  // Elevation properties
  Object.entries(tokens.elevation).forEach(([key, value]) => {
    lightProperties[`--md3-elevation-${key}`] = value
    darkProperties[`--md3-elevation-${key}`] = value
  })

  return {
    light: lightProperties,
    dark: darkProperties,
  }
}

/**
 * Generate CSS string for custom properties
 */
export const generateCSSString = (tokens: MD3Tokens): string => {
  const properties = generateCSSCustomProperties(tokens)

  const lightCSS = Object.entries(properties.light)
    .map(([property, value]) => `  ${property}: ${value};`)
    .join('\n')

  const darkCSS = Object.entries(properties.dark)
    .map(([property, value]) => `  ${property}: ${value};`)
    .join('\n')

  return `
/* MD3 Design Tokens - Light Mode */
:root {
${lightCSS}
}

/* MD3 Design Tokens - Dark Mode */
:root.dark {
${darkCSS}
}

/* Alternative dark mode selector */
[data-theme="dark"] {
${darkCSS}
}
`.trim()
}

/**
 * Apply CSS custom properties to document
 */
export const applyMD3Tokens = (
  tokens: MD3Tokens,
  mode: 'light' | 'dark' = 'light',
) => {
  const properties = generateCSSCustomProperties(tokens)
  const targetProperties = properties[mode]

  Object.entries(targetProperties).forEach(([property, value]) => {
    document.documentElement.style.setProperty(property, value)
  })
}
