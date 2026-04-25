import { isEqual, kebabCase } from 'lodash-es'
import {
  createContext,
  PropsWithChildren,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react'
import { insertStyle } from '@/utils/styled'
import {
  argbFromHex,
  hexFromArgb,
  Theme,
  themeFromSourceColor,
} from '@material/material-color-utilities'
import { useSetting } from '@nyanpasu/interface'
import { alpha, darken, lighten } from '@nyanpasu/utils'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useLocalStorage } from '@uidotdev/usehooks'

const appWindow = getCurrentWebviewWindow()

export const DEFAULT_COLOR = '#1867C0'

export enum ThemeMode {
  LIGHT = 'light',
  DARK = 'dark',
  SYSTEM = 'system',
}

type ResolvedThemeMode = ThemeMode.LIGHT | ThemeMode.DARK

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

const changeHtmlThemeMode = (mode: Omit<ThemeMode, 'system'>) => {
  const root = document.documentElement

  if (mode === ThemeMode.DARK) {
    root.classList.add(ThemeMode.DARK)
  } else {
    root.classList.remove(ThemeMode.DARK)
  }

  if (mode === ThemeMode.LIGHT) {
    root.classList.add(ThemeMode.LIGHT)
  } else {
    root.classList.remove(ThemeMode.LIGHT)
  }
}

const getSystemThemeMode = () => {
  return window.matchMedia('(prefers-color-scheme: dark)').matches
    ? ThemeMode.DARK
    : ThemeMode.LIGHT
}

const getThemeScheme = (theme: Theme, mode: ResolvedThemeMode) => {
  const scheme = theme.schemes[mode]

  return typeof scheme.toJSON === 'function' ? scheme.toJSON() : scheme
}

const applyRootStyleVar = (mode: ResolvedThemeMode, themePalette: Theme) => {
  const root = document.documentElement
  const scheme = getThemeScheme(themePalette, mode)
  const secondaryColor = hexFromArgb(scheme.secondary)
  const primaryColor = hexFromArgb(scheme.primary)
  const reactRootDom = document.getElementById('root')
  const isDarkMode = mode === ThemeMode.DARK

  root.style.setProperty(
    '--background-color',
    isDarkMode ? darken(secondaryColor, 0.95) : lighten(secondaryColor, 0.95),
  )
  root.style.setProperty(
    '--selection-color',
    isDarkMode ? '#d5d5d5' : '#f5f5f5',
  )
  root.style.setProperty(
    '--scroller-color',
    isDarkMode ? '#54545480' : '#90939980',
  )
  root.style.setProperty('--primary-main', primaryColor)
  root.style.setProperty('--background-color-alpha', alpha(primaryColor, 0.1))

  if (reactRootDom) {
    reactRootDom.classList.toggle(ThemeMode.DARK, isDarkMode)
    reactRootDom.classList.toggle(ThemeMode.LIGHT, !isDarkMode)
  }
}

const ThemeContext = createContext<{
  themePalette: Theme
  themeCssVars: string
  themeColor: string
  setThemeColor: (color: string) => Promise<void>
  themeMode: ThemeMode
  currentThemeMode: ResolvedThemeMode
  setThemeMode: (mode: ThemeMode) => Promise<void>
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
  const themeMode = useSetting('theme_mode')

  const themeColor = useSetting('theme_color')
  const [resolvedThemeMode, setResolvedThemeMode] =
    useState<ResolvedThemeMode>(getSystemThemeMode())

  const [cachedThemePalette, setCachedThemePalette] = useLocalStorage<Theme>(
    THEME_PALETTE_KEY,
    themeFromSourceColor(
      // use default color if theme color is not set
      argbFromHex(themeColor.value || DEFAULT_COLOR),
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

  useEffect(() => {
    const nextThemePalette = themeFromSourceColor(
      argbFromHex(themeColor.value || DEFAULT_COLOR),
    )

    if (!isEqual(nextThemePalette, cachedThemePalette)) {
      setCachedThemePalette(nextThemePalette)
    }

    const nextThemeCssVars = generateThemeCssVars(nextThemePalette)

    if (nextThemeCssVars !== cachedThemeCssVars) {
      setCachedThemeCssVars(nextThemeCssVars)
    }
  }, [
    themeColor.value,
    cachedThemePalette,
    cachedThemeCssVars,
    setCachedThemeCssVars,
    setCachedThemePalette,
  ])

  const setThemeColor = useCallback(
    async (color: string) => {
      if (color === themeColor.value) {
        return
      } else {
        await themeColor.upsert(color)
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

  const applyThemeMode = useCallback((mode: ResolvedThemeMode) => {
    changeHtmlThemeMode(mode)
    setResolvedThemeMode(mode)
  }, [])

  // initialize theme mode on mount
  useEffect(() => {
    const initializeTheme = async () => {
      if (themeMode.value === ThemeMode.SYSTEM) {
        // Apply a synchronous system fallback first to avoid a light flash.
        applyThemeMode(getSystemThemeMode())

        const systemTheme = await appWindow.theme()
        applyThemeMode(
          systemTheme === ThemeMode.DARK ? ThemeMode.DARK : ThemeMode.LIGHT,
        )
      } else if (
        themeMode.value === ThemeMode.LIGHT ||
        themeMode.value === ThemeMode.DARK
      ) {
        applyThemeMode(themeMode.value)
      } else {
        // Setting value may still be loading; keep current class to avoid visual flicker.
      }
    }

    initializeTheme()
  }, [applyThemeMode, themeMode.value])

  // listen to theme changed event and change html theme mode
  useEffect(() => {
    const unlisten = appWindow.onThemeChanged((e) => {
      if (themeMode.value === ThemeMode.SYSTEM) {
        applyThemeMode(
          e.payload === ThemeMode.DARK ? ThemeMode.DARK : ThemeMode.LIGHT,
        )
      }
    })

    return () => {
      unlisten.then((fn) => fn())
    }
  }, [applyThemeMode, themeMode.value])

  const setThemeMode = useCallback(
    async (mode: ThemeMode) => {
      // if theme mode is not system, change html theme mode
      if (mode !== ThemeMode.SYSTEM) {
        applyThemeMode(mode)
      }

      if (mode !== themeMode.value) {
        await themeMode.upsert(mode)
      }
    },
    [applyThemeMode, themeMode],
  )

  const currentThemeMode = useMemo<ResolvedThemeMode>(() => {
    if (themeMode.value === ThemeMode.DARK) {
      return ThemeMode.DARK
    }

    if (themeMode.value === ThemeMode.LIGHT) {
      return ThemeMode.LIGHT
    }

    return resolvedThemeMode
  }, [resolvedThemeMode, themeMode.value])

  useEffect(() => {
    applyRootStyleVar(currentThemeMode, cachedThemePalette)
  }, [cachedThemePalette, currentThemeMode])

  return (
    <ThemeContext.Provider
      value={{
        themePalette: cachedThemePalette,
        themeCssVars: cachedThemeCssVars,
        themeColor: themeColor.value || DEFAULT_COLOR,
        setThemeColor,
        themeMode: themeMode.value as ThemeMode,
        currentThemeMode,
        setThemeMode,
      }}
    >
      {children}
    </ThemeContext.Provider>
  )
}
