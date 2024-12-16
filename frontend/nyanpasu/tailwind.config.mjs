import { MUI_BREAKPOINTS } from '@nyanpasu/ui/src/materialYou/themeConsts.mjs'

const plugin = require('tailwindcss/plugin')

const getMUuiScreen = () => {
  const breakpoints = MUI_BREAKPOINTS.values

  const result = {}

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
  corePlugins: {
    preflight: true,
  },
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
    screen: getMUuiScreen(),
  },
  plugins: [
    require('tailwindcss-textshadow'),
    plugin(({ addBase }) => {
      addBase({
        '.scrollbar-hidden::-webkit-scrollbar': {
          width: '0px',
        },
      })
    }),
  ],
}
