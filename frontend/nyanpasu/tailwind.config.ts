import type { Config } from 'tailwindcss'
import createPlugin from 'tailwindcss/plugin'

module.exports = {
  content: ['./src/**/*.{tsx,ts}'],
  darkMode: 'selector',
  theme: {
    screens: {
      sm: '600px',
      md: '900px',
      lg: '1200px',
      xl: '1536px',
    },
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
  },
  plugins: [
    createPlugin(({ addBase }) => {
      addBase({
        '.scrollbar-hidden::-webkit-scrollbar': {
          width: '0px',
        },
      })
    }),
  ],
} satisfies Config
