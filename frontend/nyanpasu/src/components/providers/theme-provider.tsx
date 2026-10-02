import {
  createContext,
  PropsWithChildren,
  useCallback,
  useContext,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { insertStyle } from '@/utils/styled'
import { createWindowAdapter } from '@nyanpasu/platform'
import { useSetting } from '@nyanpasu/query'
import {
  alpha,
  createTheme,
  darken,
  DEFAULT_COLOR,
  getThemeScheme,
  hexFromArgb,
  lighten,
  ThemeMode,
  type ResolvedThemeMode,
  type Theme,
} from '@nyanpasu/theme'

const CUSTOM_THEME_KEY = 'custom-theme' as const

const THEME_PALETTE_KEY = 'theme-palette-v1' as const
const THEME_CSS_VARS_KEY = 'theme-css-vars-v1' as const

const readCachedTheme = () => {
  try {
    const palette = localStorage.getItem(THEME_PALETTE_KEY)
    const cssVars = localStorage.getItem(THEME_CSS_VARS_KEY)

    if (palette !== null && cssVars !== null) {
      return {
        palette: JSON.parse(palette) as Theme,
        cssVars: JSON.parse(cssVars) as string,
      }
    }
  } catch {
    // fall through to the default theme
  }

  return createTheme(DEFAULT_COLOR)
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

export type ThemePreview = {
  color?: string
  mode?: ThemeMode
}

const ThemeContext = createContext<{
  themePalette: Theme
  themeCssVars: string
  themeColor: string
  setThemeColor: (color: string) => Promise<void>
  themeMode: ThemeMode
  currentThemeMode: ResolvedThemeMode
  setThemeMode: (mode: ThemeMode) => Promise<void>
  setThemePreview: (preview: ThemePreview | null) => void
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
  const [appWindow] = useState(createWindowAdapter)
  const themeMode = useSetting('theme_mode')

  const themeColor = useSetting('theme_color')
  const [resolvedThemeMode, setResolvedThemeMode] =
    useState<ResolvedThemeMode>(getSystemThemeMode())

  // An unsaved color / mode shown on top of the saved settings.
  const [preview, setPreview] = useState<ThemePreview | null>(null)

  // Dragging a color changes it faster than themes rebuild; let React skip
  // the intermediate ones.
  const effectiveColor = useDeferredValue(preview?.color ?? themeColor.value)
  const effectiveMode = preview?.mode ?? themeMode.value

  // The theme of the last session paints the window until the settings have
  // loaded; `theme_color` is always present once they have.
  const [cachedTheme] = useState(readCachedTheme)

  const theme = useMemo(
    () =>
      effectiveColor === undefined ? cachedTheme : createTheme(effectiveColor),
    [effectiveColor, cachedTheme],
  )

  // Only the saved color is cached for the next launch, never a preview
  // (including the deferred frame right after a preview is cleared).
  const isSavedTheme = effectiveColor === themeColor.value

  // automatically insert custom theme css vars into document head
  useEffect(() => {
    insertStyle(CUSTOM_THEME_KEY, theme.cssVars)
  }, [theme.cssVars])

  useEffect(() => {
    if (theme === cachedTheme || !isSavedTheme) {
      return
    }

    try {
      localStorage.setItem(THEME_PALETTE_KEY, JSON.stringify(theme.palette))
      localStorage.setItem(THEME_CSS_VARS_KEY, JSON.stringify(theme.cssVars))
    } catch {
      // ignore quota / security errors
    }
  }, [theme, cachedTheme, isSavedTheme])

  // The setting hooks return new objects every render; the setters read
  // them through refs so the context value stays stable.
  const themeColorRef = useRef(themeColor)
  themeColorRef.current = themeColor

  const themeModeRef = useRef(themeMode)
  themeModeRef.current = themeMode

  const setThemeColor = useCallback(async (color: string) => {
    if (color !== themeColorRef.current.value) {
      await themeColorRef.current.upsert(color)
    }
  }, [])

  const setThemePreview = useCallback((next: ThemePreview | null) => {
    setPreview((current) =>
      current?.color === next?.color && current?.mode === next?.mode
        ? current
        : next,
    )
  }, [])

  const applyThemeMode = useCallback((mode: ResolvedThemeMode) => {
    changeHtmlThemeMode(mode)
    setResolvedThemeMode(mode)
  }, [])

  // initialize theme mode on mount
  useEffect(() => {
    const initializeTheme = async () => {
      if (effectiveMode === ThemeMode.SYSTEM) {
        // Apply a synchronous system fallback first to avoid a light flash.
        applyThemeMode(getSystemThemeMode())

        if (!appWindow) return
        const systemTheme = await appWindow.theme()
        applyThemeMode(
          systemTheme === ThemeMode.DARK ? ThemeMode.DARK : ThemeMode.LIGHT,
        )
      } else if (
        effectiveMode === ThemeMode.LIGHT ||
        effectiveMode === ThemeMode.DARK
      ) {
        applyThemeMode(effectiveMode as ResolvedThemeMode)
      } else {
        // Setting value may still be loading; keep current class to avoid visual flicker.
      }
    }

    initializeTheme()
  }, [appWindow, applyThemeMode, effectiveMode])

  // listen to theme changed event and change html theme mode
  useEffect(() => {
    if (!appWindow) {
      const media = window.matchMedia('(prefers-color-scheme: dark)')
      const update = () => {
        if (effectiveMode === ThemeMode.SYSTEM)
          applyThemeMode(getSystemThemeMode())
      }
      media.addEventListener('change', update)
      return () => media.removeEventListener('change', update)
    }
    const unlisten = appWindow.onThemeChanged((theme) => {
      if (effectiveMode === ThemeMode.SYSTEM) {
        applyThemeMode(
          theme === ThemeMode.DARK ? ThemeMode.DARK : ThemeMode.LIGHT,
        )
      }
    })

    return () => {
      unlisten.then((fn) => fn())
    }
  }, [appWindow, applyThemeMode, effectiveMode])

  const setThemeMode = useCallback(
    async (mode: ThemeMode) => {
      // if theme mode is not system, change html theme mode
      if (mode !== ThemeMode.SYSTEM) {
        applyThemeMode(mode)
      }

      if (mode !== themeModeRef.current.value) {
        await themeModeRef.current.upsert(mode)
      }
    },
    [applyThemeMode],
  )

  const currentThemeMode = useMemo<ResolvedThemeMode>(() => {
    if (effectiveMode === ThemeMode.DARK) {
      return ThemeMode.DARK
    }

    if (effectiveMode === ThemeMode.LIGHT) {
      return ThemeMode.LIGHT
    }

    return resolvedThemeMode
  }, [resolvedThemeMode, effectiveMode])

  useEffect(() => {
    applyRootStyleVar(currentThemeMode, theme.palette)
  }, [theme.palette, currentThemeMode])

  const color = themeColor.value || DEFAULT_COLOR

  const value = useMemo(
    () => ({
      themePalette: theme.palette,
      themeCssVars: theme.cssVars,
      themeColor: color,
      setThemeColor,
      themeMode: themeMode.value as ThemeMode,
      currentThemeMode,
      setThemeMode,
      setThemePreview,
    }),
    [
      theme,
      color,
      setThemeColor,
      themeMode.value,
      currentThemeMode,
      setThemeMode,
      setThemePreview,
    ],
  )

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}
