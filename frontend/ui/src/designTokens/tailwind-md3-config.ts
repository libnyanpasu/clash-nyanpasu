import type { Config } from 'tailwindcss'
import { MUI_BREAKPOINTS } from '../materialYou/themeConsts.mjs'

/**
 * MD3 Design Tokens for Tailwind CSS
 * Extends Tailwind with Material Design 3 tokens via CSS custom properties
 */

// Convert MUI breakpoints to Tailwind screens format
const getMD3Screens = () => {
  const breakpoints = MUI_BREAKPOINTS.values as Record<string, number>
  const result = {} as Record<string, string>

  for (const key in breakpoints) {
    if (Object.prototype.hasOwnProperty.call(breakpoints, key)) {
      result[key] = `${breakpoints[key]}px`
    }
  }

  return result
}

// MD3 Color tokens mapped to CSS custom properties
const md3Colors = {
  // Primary colors
  primary: 'var(--md3-color-primary)',
  'on-primary': 'var(--md3-color-on-primary)',
  'primary-container': 'var(--md3-color-primary-container)',
  'on-primary-container': 'var(--md3-color-on-primary-container)',

  // Secondary colors
  secondary: 'var(--md3-color-secondary)',
  'on-secondary': 'var(--md3-color-on-secondary)',
  'secondary-container': 'var(--md3-color-secondary-container)',
  'on-secondary-container': 'var(--md3-color-on-secondary-container)',

  // Tertiary colors
  tertiary: 'var(--md3-color-tertiary)',
  'on-tertiary': 'var(--md3-color-on-tertiary)',
  'tertiary-container': 'var(--md3-color-tertiary-container)',
  'on-tertiary-container': 'var(--md3-color-on-tertiary-container)',

  // Error colors
  error: 'var(--md3-color-error)',
  'on-error': 'var(--md3-color-on-error)',
  'error-container': 'var(--md3-color-error-container)',
  'on-error-container': 'var(--md3-color-on-error-container)',

  // Surface colors
  surface: 'var(--md3-color-surface)',
  'on-surface': 'var(--md3-color-on-surface)',
  'surface-variant': 'var(--md3-color-surface-variant)',
  'on-surface-variant': 'var(--md3-color-on-surface-variant)',
  'surface-dim': 'var(--md3-color-surface-dim)',
  'surface-bright': 'var(--md3-color-surface-bright)',
  'surface-container-lowest': 'var(--md3-color-surface-container-lowest)',
  'surface-container-low': 'var(--md3-color-surface-container-low)',
  'surface-container': 'var(--md3-color-surface-container)',
  'surface-container-high': 'var(--md3-color-surface-container-high)',
  'surface-container-highest': 'var(--md3-color-surface-container-highest)',

  // Background colors
  background: 'var(--md3-color-background)',
  'on-background': 'var(--md3-color-on-background)',

  // Outline colors
  outline: 'var(--md3-color-outline)',
  'outline-variant': 'var(--md3-color-outline-variant)',

  // Other colors
  shadow: 'var(--md3-color-shadow)',
  scrim: 'var(--md3-color-scrim)',
  'inverse-surface': 'var(--md3-color-inverse-surface)',
  'inverse-on-surface': 'var(--md3-color-inverse-on-surface)',
  'inverse-primary': 'var(--md3-color-inverse-primary)',
}

// MD3 Spacing tokens
const md3Spacing = {
  '0': 'var(--md3-spacing-0)',
  '1': 'var(--md3-spacing-1)',
  '2': 'var(--md3-spacing-2)',
  '3': 'var(--md3-spacing-3)',
  '4': 'var(--md3-spacing-4)',
  '5': 'var(--md3-spacing-5)',
  '6': 'var(--md3-spacing-6)',
  '8': 'var(--md3-spacing-8)',
  '10': 'var(--md3-spacing-10)',
  '12': 'var(--md3-spacing-12)',
  '16': 'var(--md3-spacing-16)',
  '20': 'var(--md3-spacing-20)',
  '24': 'var(--md3-spacing-24)',
  '32': 'var(--md3-spacing-32)',
}

// MD3 Border radius tokens
const md3BorderRadius = {
  none: 'var(--md3-radius-none)',
  xs: 'var(--md3-radius-xs)',
  sm: 'var(--md3-radius-sm)',
  md: 'var(--md3-radius-md)',
  lg: 'var(--md3-radius-lg)',
  xl: 'var(--md3-radius-xl)',
  '2xl': 'var(--md3-radius-2xl)',
  '3xl': 'var(--md3-radius-3xl)',
  full: 'var(--md3-radius-full)',
}

// MD3 Box shadow tokens
const md3BoxShadow = {
  '0': 'var(--md3-elevation-0)',
  '1': 'var(--md3-elevation-1)',
  '2': 'var(--md3-elevation-2)',
  '3': 'var(--md3-elevation-3)',
  '4': 'var(--md3-elevation-4)',
  '5': 'var(--md3-elevation-5)',
}

/**
 * Tailwind configuration with MD3 design tokens
 */
export const md3TailwindConfig: Partial<Config> = {
  theme: {
    extend: {
      // MD3 Colors
      colors: {
        ...md3Colors,
        // Legacy support for existing colors
        scroller: 'var(--scroller-color)',
        container: 'var(--background-color)',
      },

      // MD3 Spacing
      spacing: md3Spacing,

      // MD3 Border Radius
      borderRadius: md3BorderRadius,

      // MD3 Box Shadow (Elevation)
      boxShadow: md3BoxShadow,

      // Font Family
      fontFamily: {
        md3: 'var(--md3-font-family)',
      },

      // Existing custom utilities
      maxHeight: {
        '1/8': 'calc(100vh / 8)',
      },
      zIndex: {
        top: 100000,
      },
      animation: {
        marquee: 'marquee 4s linear infinite',
      },
      keyframes: {
        marquee: {
          '0%': { transform: 'translateX(100%)' },
          '100%': { transform: 'translateX(-100%)' },
        },
      },
    },
    screens: getMD3Screens(),
  },
}

/**
 * Utility classes for common MD3 patterns
 */
export const md3UtilityClasses = {
  // Surface containers
  '.surface-container-lowest': {
    backgroundColor: 'var(--md3-color-surface-container-lowest)',
    color: 'var(--md3-color-on-surface)',
  },
  '.surface-container-low': {
    backgroundColor: 'var(--md3-color-surface-container-low)',
    color: 'var(--md3-color-on-surface)',
  },
  '.surface-container': {
    backgroundColor: 'var(--md3-color-surface-container)',
    color: 'var(--md3-color-on-surface)',
  },
  '.surface-container-high': {
    backgroundColor: 'var(--md3-color-surface-container-high)',
    color: 'var(--md3-color-on-surface)',
  },
  '.surface-container-highest': {
    backgroundColor: 'var(--md3-color-surface-container-highest)',
    color: 'var(--md3-color-on-surface)',
  },

  // Primary containers
  '.primary-container': {
    backgroundColor: 'var(--md3-color-primary-container)',
    color: 'var(--md3-color-on-primary-container)',
  },

  // Secondary containers
  '.secondary-container': {
    backgroundColor: 'var(--md3-color-secondary-container)',
    color: 'var(--md3-color-on-secondary-container)',
  },

  // Error containers
  '.error-container': {
    backgroundColor: 'var(--md3-color-error-container)',
    color: 'var(--md3-color-on-error-container)',
  },

  // Material You button styles
  '.btn-filled': {
    backgroundColor: 'var(--md3-color-primary)',
    color: 'var(--md3-color-on-primary)',
    borderRadius: 'var(--md3-radius-full)',
    padding: '10px 24px',
    fontWeight: '500',
    transition: 'all 0.2s ease',
    '&:hover': {
      boxShadow: 'var(--md3-elevation-1)',
    },
  },

  '.btn-outlined': {
    backgroundColor: 'transparent',
    color: 'var(--md3-color-primary)',
    border: '1px solid var(--md3-color-outline)',
    borderRadius: 'var(--md3-radius-full)',
    padding: '10px 24px',
    fontWeight: '500',
    transition: 'all 0.2s ease',
    '&:hover': {
      backgroundColor: 'var(--md3-color-primary-container)',
      color: 'var(--md3-color-on-primary-container)',
    },
  },

  '.btn-text': {
    backgroundColor: 'transparent',
    color: 'var(--md3-color-primary)',
    borderRadius: 'var(--md3-radius-full)',
    padding: '10px 12px',
    fontWeight: '500',
    transition: 'all 0.2s ease',
    '&:hover': {
      backgroundColor: 'var(--md3-color-primary-container)',
      color: 'var(--md3-color-on-primary-container)',
    },
  },

  // Card styles
  '.card-elevated': {
    backgroundColor: 'var(--md3-color-surface-container-low)',
    color: 'var(--md3-color-on-surface)',
    borderRadius: 'var(--md3-radius-lg)',
    boxShadow: 'var(--md3-elevation-1)',
  },

  '.card-filled': {
    backgroundColor: 'var(--md3-color-surface-container-highest)',
    color: 'var(--md3-color-on-surface)',
    borderRadius: 'var(--md3-radius-lg)',
  },

  '.card-outlined': {
    backgroundColor: 'var(--md3-color-surface)',
    color: 'var(--md3-color-on-surface)',
    border: '1px solid var(--md3-color-outline-variant)',
    borderRadius: 'var(--md3-radius-lg)',
  },
}

export default md3TailwindConfig
