import { useAtomValue, useSetAtom } from 'jotai'
import { PropsWithChildren, useEffect, useMemo } from 'react'
import { themeMode as themeModeAtom } from '@/store'
import { alpha, darken, lighten, Theme, useColorScheme } from '@mui/material'
import { ThemeProvider } from '@mui/material/styles'
import { useSetting } from '@nyanpasu/interface'
import { cn, createMDYTheme } from '@nyanpasu/ui'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

export const DEFAULT_COLOR = '#1867c0'

export const DEFAULT_FONT_FAMILY = `"Roboto", "Helvetica", "Arial", sans-serif, "Color Emoji Flags"," Color Emoji"`

const appWindow = getCurrentWebviewWindow()

const applyRootStyleVar = (mode: 'light' | 'dark', theme: Theme) => {
  const root = document.documentElement
  const palette = theme.palette

  const isLightMode = mode !== 'light'
  root.className = cn(mode === 'dark' ? 'dark' : 'light')
  const backgroundColor = isLightMode
    ? darken(palette.secondary.dark, 0.95)
    : lighten(palette.secondary.light, 0.95)

  const selectionColor = isLightMode ? '#d5d5d5' : '#f5f5f5'
  const scrollerColor = isLightMode ? '#54545480' : '#90939980'

  root.style.setProperty('--background-color', backgroundColor)
  root.style.setProperty('--selection-color', selectionColor)
  root.style.setProperty('--scroller-color', scrollerColor)
  root.style.setProperty('--primary-main', palette.primary.main)
  root.style.setProperty(
    '--background-color-alpha',
    alpha(palette.primary.main, 0.1),
  )

  const reactRootDom = document.getElementById('root')
  if (reactRootDom) {
    reactRootDom.className = cn(mode === 'dark' ? 'dark' : 'light')
  }
}

/**
 * custom theme
 */
export const useCustomTheme = () => {
  const themeMode = useAtomValue(themeModeAtom)

  const { value: themeColor } = useSetting('theme_color')

  const theme = useMemo(() => {
    const color = themeColor || DEFAULT_COLOR

    const mergedTheme = createMDYTheme(color)

    applyRootStyleVar(themeMode, mergedTheme)

    return mergedTheme
  }, [themeColor, themeMode])

  return { theme }
}

export const ThemeModeProvider = ({ children }: PropsWithChildren) => {
  const { value: themeMode } = useSetting('theme_mode')

  const { theme } = useCustomTheme()

  const setThemeMode = useSetAtom(themeModeAtom)

  const { setMode } = useColorScheme()

  useEffect(() => {
    if (themeMode === 'system') {
      appWindow.theme().then((m) => {
        if (m) {
          setThemeMode(m)
          setMode(m)
        }
      })
    } else {
      const chosenThemeMode = (themeMode as 'light' | 'dark') || 'light'
      setThemeMode(chosenThemeMode)
      setMode(chosenThemeMode)
    }

    const unlisten = appWindow.onThemeChanged((e) => {
      if (themeMode === 'system') {
        setThemeMode(e.payload)
        setMode(e.payload)
      }
    })

    return () => {
      unlisten.then((fn) => fn())
    }
  }, [setMode, setThemeMode, themeMode])

  return <ThemeProvider theme={theme}>{children}</ThemeProvider>
}
