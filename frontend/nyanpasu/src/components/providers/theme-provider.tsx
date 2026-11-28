import {
  createContext,
  PropsWithChildren,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { insertStyle } from '@/utils/styled'
import { createWindowAdapter } from '@nyanpasu/platform'
import { useSetting, useSettings } from '@nyanpasu/query'
import type { ApplicationSettings_Serialize } from '@nyanpasu/rpc/types'
import {
  createTheme,
  DEFAULT_COLOR,
  DEFAULT_CONTRAST_LEVEL,
  DEFAULT_THEME_SPEC_VERSION,
  DEFAULT_THEME_VARIANT,
  isValidThemePalette,
  ThemeMode,
  type ResolvedThemeMode,
  type Theme,
  type ThemeSpecVersion,
  type ThemeVariant,
} from '@nyanpasu/theme'

const CUSTOM_THEME_KEY = 'custom-theme' as const

export const THEME_PALETTE_KEY = 'theme-palette-v2' as const
export const THEME_CSS_VARS_KEY = 'theme-css-vars-v2' as const

// TODO(theme-preload): Theme bootstrap currently relies on a client-side
// localStorage cache read inside the React tree. Once the preload architecture
// is designed, unify initial theme/CSS-var injection into either backend
// webview startup injection or `__root.tsx`'s `beforeLoad` hook.
const readCachedTheme = () => {
  try {
    const rawPalette = localStorage.getItem(THEME_PALETTE_KEY)
    const rawCssVars = localStorage.getItem(THEME_CSS_VARS_KEY)

    if (rawPalette !== null && rawCssVars !== null) {
      const palette: unknown = JSON.parse(rawPalette)
      const cssVars: unknown = JSON.parse(rawCssVars)

      if (
        isValidThemePalette(palette) &&
        typeof cssVars === 'string' &&
        cssVars.includes('--color-md-surface-container-lowest:')
      ) {
        return {
          palette,
          cssVars,
        }
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

const applyRootThemeClass = (mode: ResolvedThemeMode) => {
  const reactRootDom = document.getElementById('root')
  const isDarkMode = mode === ThemeMode.DARK

  if (reactRootDom) {
    reactRootDom.classList.toggle(ThemeMode.DARK, isDarkMode)
    reactRootDom.classList.toggle(ThemeMode.LIGHT, !isDarkMode)
  }
}

type SettingsThemeVariant = ApplicationSettings_Serialize['theme_variant']

/** The saved values every color role is generated from. */
export type ThemeOptions = {
  color: string
  variant: ThemeVariant
  contrastLevel: number
  specVersion: ThemeSpecVersion
}

// Settings spell the variants in snake_case, the theme package in camelCase.
const toThemeVariant = (variant: SettingsThemeVariant): ThemeVariant =>
  variant.replace(/_([a-z])/g, (_, letter: string) =>
    letter.toUpperCase(),
  ) as ThemeVariant

const toSettingsVariant = (variant: ThemeVariant): SettingsThemeVariant =>
  variant.replace(
    /[A-Z]/g,
    (letter) => `_${letter.toLowerCase()}`,
  ) as SettingsThemeVariant

const ThemeContext = createContext<{
  themePalette: Theme
  themeCssVars: string
  themeOptions: ThemeOptions
  setThemeOptions: (options: ThemeOptions) => Promise<void>
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

export function ExperimentalThemeProvider({
  enabled = true,
  children,
}: PropsWithChildren<{ enabled?: boolean }>) {
  const [appWindow] = useState(createWindowAdapter)
  const themeMode = useSetting('theme_mode', { enabled })

  const themeColor = useSetting('theme_color', { enabled })
  const themeVariant = useSetting('theme_variant', { enabled })
  const themeContrastLevel = useSetting('theme_contrast_level', { enabled })
  const themeSpecVersion = useSetting('theme_spec_version', { enabled })
  const { upsert: patchSettings } = useSettings({ enabled })

  const [resolvedThemeMode, setResolvedThemeMode] =
    useState<ResolvedThemeMode>(getSystemThemeMode())

  const effectiveMode = themeMode.value

  const color = themeColor.value
  const variant =
    themeVariant.value === undefined
      ? DEFAULT_THEME_VARIANT
      : toThemeVariant(themeVariant.value)
  const contrastLevel = themeContrastLevel.value ?? DEFAULT_CONTRAST_LEVEL
  const specVersion = themeSpecVersion.value ?? DEFAULT_THEME_SPEC_VERSION

  // The theme of the last session paints the window until the settings have
  // loaded; `theme_color` is always present once they have.
  const [cachedTheme] = useState(readCachedTheme)

  const theme = useMemo(
    () =>
      color === undefined
        ? cachedTheme
        : createTheme(color, { variant, contrastLevel, specVersion }),
    [color, variant, contrastLevel, specVersion, cachedTheme],
  )

  // automatically insert custom theme css vars into document head
  useEffect(() => {
    insertStyle(CUSTOM_THEME_KEY, theme.cssVars)
  }, [theme.cssVars])

  useEffect(() => {
    if (theme === cachedTheme) {
      return
    }

    try {
      localStorage.setItem(THEME_PALETTE_KEY, JSON.stringify(theme.palette))
      localStorage.setItem(THEME_CSS_VARS_KEY, JSON.stringify(theme.cssVars))
    } catch {
      // ignore quota / security errors
    }
  }, [theme, cachedTheme])

  const themeOptions = useMemo<ThemeOptions>(
    () => ({
      color: color || DEFAULT_COLOR,
      variant,
      contrastLevel,
      specVersion,
    }),
    [color, variant, contrastLevel, specVersion],
  )

  // The setting hooks return new objects every render; the setters read
  // them through refs so the context value stays stable.
  const themeOptionsRef = useRef(themeOptions)
  themeOptionsRef.current = themeOptions

  const patchSettingsRef = useRef(patchSettings)
  patchSettingsRef.current = patchSettings

  const themeModeRef = useRef(themeMode)
  themeModeRef.current = themeMode

  // One patch saves every changed option, so the theme never rebuilds from
  // a half-applied set.
  const setThemeOptions = useCallback(async (next: ThemeOptions) => {
    const saved = themeOptionsRef.current
    const patch: Parameters<typeof patchSettings.mutateAsync>[0] = {}

    if (next.color.toLowerCase() !== saved.color.toLowerCase()) {
      patch.theme_color = next.color
    }

    if (next.variant !== saved.variant) {
      patch.theme_variant = toSettingsVariant(next.variant)
    }

    if (next.contrastLevel !== saved.contrastLevel) {
      patch.theme_contrast_level = next.contrastLevel
    }

    if (next.specVersion !== saved.specVersion) {
      patch.theme_spec_version = next.specVersion
    }

    if (Object.keys(patch).length > 0) {
      await patchSettingsRef.current.mutateAsync(patch)
    }
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
    applyRootThemeClass(currentThemeMode)
  }, [currentThemeMode])

  const value = useMemo(
    () => ({
      themePalette: theme.palette,
      themeCssVars: theme.cssVars,
      themeOptions,
      setThemeOptions,
      themeMode: themeMode.value as ThemeMode,
      currentThemeMode,
      setThemeMode,
    }),
    [
      theme,
      themeOptions,
      setThemeOptions,
      themeMode.value,
      currentThemeMode,
      setThemeMode,
    ],
  )

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}
