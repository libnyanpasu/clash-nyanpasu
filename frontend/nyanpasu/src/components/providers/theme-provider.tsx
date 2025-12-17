import { isEqual, kebabCase } from 'lodash-es'
import {
  createContext,
  PropsWithChildren,
  useCallback,
  useContext,
  useEffect,
} from 'react'
import { insertStyle } from '@/utils/styled'
import {
  argbFromHex,
  hexFromArgb,
  Theme,
  themeFromSourceColor,
} from '@material/material-color-utilities'
import { useSetting } from '@nyanpasu/interface'
import { useLocalStorage } from '@uidotdev/usehooks'

export const DEFAULT_COLOR = '#1867C0'

const CUSTOM_THEME_KEY = 'custom-theme' as const

const THEME_PALETTE_KEY = 'theme-palette-v1' as const
const THEME_CSS_VARS_KEY = 'theme-css-vars-v1' as const

const generateThemeCssVars = ({ schemes }: Theme) => {
  let lightCssVars = ':root{'
  let darkCssVars = ':root.dark{'

  Object.entries(schemes).forEach(([mode, scheme]) => {
    let inputScheme

    // Safely convert scheme to JSON if possible, otherwise use as-is
    if (typeof scheme.toJSON === 'function') {
      inputScheme = scheme.toJSON()
    } else {
      inputScheme = scheme
    }

    Object.entries(inputScheme).forEach(([key, value]) => {
      if (mode === 'light') {
        lightCssVars += `--color-md-${kebabCase(key)}: ${hexFromArgb(value)};`
      } else {
        darkCssVars += `--color-md-${kebabCase(key)}: ${hexFromArgb(value)};`
      }
    })
  })

  lightCssVars += '}'
  darkCssVars += '}'

  return lightCssVars + darkCssVars
}

const ThemeContext = createContext<{
  themePalette: Theme
  themeCssVars: string
  themeColor: string
  setTheme: (color: string) => void
} | null>(null)

export function useExperimentalThemeContext() {
  const context = useContext(ThemeContext)

  if (!context) {
    throw new Error(
      'useExperimentalThemeContext must be used within a ExperimentalThemeProvider',
    )
  }

  return context
}

export function ExperimentalThemeProvider({ children }: PropsWithChildren) {
  const { value: themeColor } = useSetting('theme_color')

  const [cachedThemePalette, setCachedThemePalette] = useLocalStorage<Theme>(
    THEME_PALETTE_KEY,
    themeFromSourceColor(
      // use default color if theme color is not set
      argbFromHex(themeColor || DEFAULT_COLOR),
    ),
  )

  const [cachedThemeCssVars, setCachedThemeCssVars] = useLocalStorage<string>(
    THEME_CSS_VARS_KEY,
    // initialize theme css vars from cached theme palette
    generateThemeCssVars(cachedThemePalette),
  )

  // automatically insert custom theme css vars into document head
  useEffect(() => {
    insertStyle(CUSTOM_THEME_KEY, cachedThemeCssVars)
  }, [cachedThemeCssVars])

  const setTheme = useCallback(
    (color: string) => {
      if (color === themeColor) {
        return
      }

      const materialColor = themeFromSourceColor(
        // use default color if theme color is not set
        argbFromHex(color || DEFAULT_COLOR),
      )

      if (isEqual(materialColor, cachedThemePalette)) {
        return
      } else {
        setCachedThemePalette(materialColor)
      }

      const themeCssVars = generateThemeCssVars(materialColor)
      setCachedThemeCssVars(themeCssVars)
    },
    [
      themeColor,
      cachedThemePalette,
      setCachedThemeCssVars,
      setCachedThemePalette,
    ],
  )

  return (
    <ThemeContext.Provider
      value={{
        themePalette: cachedThemePalette,
        themeCssVars: cachedThemeCssVars,
        themeColor: themeColor || DEFAULT_COLOR,
        setTheme,
      }}
    >
      {children}
    </ThemeContext.Provider>
  )
}
