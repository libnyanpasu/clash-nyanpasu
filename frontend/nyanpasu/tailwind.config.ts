import type { Config } from 'tailwindcss'
import createPlugin from 'tailwindcss/plugin'
import {
  md3TailwindConfig,
  md3UtilityClasses,
} from '@nyanpasu/ui/src/designTokens/tailwind-md3-config'
import { MUI_BREAKPOINTS } from '@nyanpasu/ui/src/materialYou/themeConsts.mjs'

const getMUIScreen = () => {
  const breakpoints = MUI_BREAKPOINTS.values as Record<string, number>

  const result = {} as Record<string, string>

  for (const key in breakpoints) {
    if (Object.prototype.hasOwnProperty.call(breakpoints, key)) {
      result[key] = `${breakpoints[key]}px`
    }
  }

  return result
}

/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ['./src/**/*.{tsx,ts}', '../ui/**/*.{tsx,ts}'],
  darkMode: 'selector',
  theme: {
    extend: {
      // Merge MD3 configuration
      ...md3TailwindConfig.theme?.extend,

      // Keep existing custom utilities
      maxHeight: {
        '1/8': 'calc(100vh / 8)',
        ...md3TailwindConfig.theme?.extend?.maxHeight,
      },
      zIndex: {
        top: 100000,
        ...md3TailwindConfig.theme?.extend?.zIndex,
      },
      animation: {
        marquee: 'marquee 4s linear infinite',
        ...md3TailwindConfig.theme?.extend?.animation,
      },
      keyframes: {
        marquee: {
          '0%': { transform: 'translateX(100%)' },
          '100%': { transform: 'translateX(-100%)' },
        },
        ...md3TailwindConfig.theme?.extend?.keyframes,
      },
      colors: {
        // Legacy colors for backward compatibility
        scroller: 'var(--scroller-color)',
        container: 'var(--background-color)',
        // MD3 colors are included via md3TailwindConfig
        ...md3TailwindConfig.theme?.extend?.colors,
      },
    },
    screens: getMUIScreen(),
  },
  plugins: [
    createPlugin(({ addBase, addUtilities }) => {
      addBase({
        '.scrollbar-hidden::-webkit-scrollbar': {
          width: '0px',
        },
      })

      // Add MD3 utility classes
      addUtilities(md3UtilityClasses)
    }),
  ],
} satisfies Config
