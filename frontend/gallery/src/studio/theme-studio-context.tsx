import { MotionConfig } from 'motion/react'
import {
  createContext,
  useCallback,
  useContext,
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
  type PropsWithChildren,
} from 'react'
import {
  createTheme,
  DEFAULT_COLOR,
  DEFAULT_CONTRAST_LEVEL,
  DEFAULT_THEME_SPEC_VERSION,
  DEFAULT_THEME_VARIANT,
  generateScopedThemeCssVars,
  ThemeMode,
  type ResolvedThemeMode,
  type Theme,
  type ThemeSpecVersion,
  type ThemeVariant,
} from '@nyanpasu/theme'

export type GalleryViewMode = 'light' | 'dark' | 'split'

export type GalleryViewportPreset = 'full' | 'expanded' | 'medium' | 'compact'

export const VIEWPORT_WIDTHS: Record<GalleryViewportPreset, number | null> = {
  full: null,
  expanded: 1280,
  medium: 840,
  compact: 390,
}

export interface ThemeStudioState {
  seedColor: string
  variant: ThemeVariant
  contrastLevel: number
  specVersion: ThemeSpecVersion
  mode: GalleryViewMode
  resolvedMode: ResolvedThemeMode
  viewport: GalleryViewportPreset
  reducedMotion: boolean
  reducedTransparency: boolean
  xraySlots: boolean
  palette: Theme
  setSeedColor: (color: string) => void
  setVariant: (variant: ThemeVariant) => void
  setContrastLevel: (level: number) => void
  setSpecVersion: (specVersion: ThemeSpecVersion) => void
  setMode: (mode: GalleryViewMode) => void
  setViewport: (viewport: GalleryViewportPreset) => void
  setReducedMotion: (value: boolean) => void
  setReducedTransparency: (value: boolean) => void
  setXraySlots: (value: boolean) => void
  resetTheme: () => void
}

const STYLE_ELEMENT_ID = 'nyanpasu-gallery-theme-vars'

const ThemeStudioContext = createContext<ThemeStudioState | null>(null)

export function useThemeStudio(): ThemeStudioState {
  const context = useContext(ThemeStudioContext)

  if (!context) {
    throw new Error('useThemeStudio must be used within ThemeStudioProvider')
  }

  return context
}

function getInitialMode(): GalleryViewMode {
  if (typeof window === 'undefined') return 'light'
  return window.matchMedia('(prefers-color-scheme: dark)').matches
    ? 'dark'
    : 'light'
}

export function ThemeStudioProvider({ children }: PropsWithChildren) {
  const [seedColor, setSeedColor] = useState(DEFAULT_COLOR)
  const [variant, setVariant] = useState<ThemeVariant>(DEFAULT_THEME_VARIANT)
  const [contrastLevel, setContrastLevel] = useState(DEFAULT_CONTRAST_LEVEL)
  const [specVersion, setSpecVersion] = useState<ThemeSpecVersion>(
    DEFAULT_THEME_SPEC_VERSION,
  )
  const [mode, setMode] = useState<GalleryViewMode>(getInitialMode)
  const [viewport, setViewport] = useState<GalleryViewportPreset>('full')
  const [reducedMotion, setReducedMotion] = useState(false)
  const [reducedTransparency, setReducedTransparency] = useState(false)
  const [xraySlots, setXraySlots] = useState(false)

  const deferredSeed = useDeferredValue(seedColor)

  const generated = useMemo(
    () =>
      createTheme(deferredSeed, {
        variant,
        contrastLevel,
        specVersion,
      }),
    [deferredSeed, variant, contrastLevel, specVersion],
  )

  const resolvedMode: ResolvedThemeMode =
    mode === 'dark' ? ThemeMode.DARK : ThemeMode.LIGHT

  useEffect(() => {
    const root = document.documentElement
    root.classList.toggle('dark', resolvedMode === ThemeMode.DARK)
    root.classList.toggle('light', resolvedMode === ThemeMode.LIGHT)
  }, [resolvedMode])

  useEffect(() => {
    let styleEl = document.getElementById(
      STYLE_ELEMENT_ID,
    ) as HTMLStyleElement | null

    if (!styleEl) {
      styleEl = document.createElement('style')
      styleEl.id = STYLE_ELEMENT_ID
      document.head.appendChild(styleEl)
    }

    const scopedLight = generateScopedThemeCssVars(
      generated.palette,
      '.gallery-theme-light',
      ThemeMode.LIGHT,
    )
    const scopedDark = generateScopedThemeCssVars(
      generated.palette,
      '.gallery-theme-dark',
      ThemeMode.DARK,
    )

    styleEl.textContent = `${generated.cssVars}${scopedLight}${scopedDark}`
  }, [generated])

  const resetTheme = useCallback(() => {
    setSeedColor(DEFAULT_COLOR)
    setVariant(DEFAULT_THEME_VARIANT)
    setContrastLevel(DEFAULT_CONTRAST_LEVEL)
    setSpecVersion(DEFAULT_THEME_SPEC_VERSION)
    setViewport('full')
    setReducedMotion(false)
    setReducedTransparency(false)
    setXraySlots(false)
  }, [])

  const value = useMemo<ThemeStudioState>(
    () => ({
      seedColor,
      variant,
      contrastLevel,
      specVersion,
      mode,
      resolvedMode,
      viewport,
      reducedMotion,
      reducedTransparency,
      xraySlots,
      palette: generated.palette,
      setSeedColor,
      setVariant,
      setContrastLevel,
      setSpecVersion,
      setMode,
      setViewport,
      setReducedMotion,
      setReducedTransparency,
      setXraySlots,
      resetTheme,
    }),
    [
      seedColor,
      variant,
      contrastLevel,
      specVersion,
      mode,
      resolvedMode,
      viewport,
      reducedMotion,
      reducedTransparency,
      xraySlots,
      generated.palette,
      resetTheme,
    ],
  )

  return (
    <ThemeStudioContext.Provider value={value}>
      <MotionConfig reducedMotion={reducedMotion ? 'always' : 'user'}>
        {children}
      </MotionConfig>
    </ThemeStudioContext.Provider>
  )
}
