import type { Config } from 'tailwindcss'
import createPlugin from 'tailwindcss/plugin'
import { MUI_BREAKPOINTS } from '@nyanpasu/ui/src/materialYou/themeConsts.mjs'
import mdplugin from '@libnyanpasu/material-design-tailwind'

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

const tailwindConfig: Config = {
  content: [
    './node_modules/@libnyanpasu/material-design-react/**/*',
    './src/**/*.{tsx,ts}',
    '../ui/**/*.{tsx,ts}',
  ],
  darkMode: 'class',
  theme: {
    extend: {
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
      colors: {
        scroller: 'var(--scroller-color)',
        container: 'var(--background-color)',
      },
    },
    screen: getMUIScreen(),
  },
  plugins: [
    mdplugin,
    createPlugin(({ addBase }) => {
      addBase({
        '.scrollbar-hidden::-webkit-scrollbar': {
          width: '0px',
        },
      })
    }),
  ],
}

export default tailwindConfig
