import { useWhyDidYouUpdate } from 'ahooks'
import { useAtomValue, useSetAtom } from 'jotai'
import { mergeWith } from 'lodash-es'
import { useEffect, useMemo } from 'react'
import { defaultTheme } from '@/pages/-theme'
import { themeMode as themeModeAtom } from '@/store'
import { alpha, darken, lighten, Theme, useColorScheme } from '@mui/material'
import { useNyanpasu } from '@nyanpasu/interface'
import { cn, createMDYTheme } from '@nyanpasu/ui'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

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
  const { nyanpasuConfig } = useNyanpasu()
  const themeMode = useAtomValue(themeModeAtom)

  useWhyDidYouUpdate('useCustomTheme', { nyanpasuConfig, themeMode })

  const theme = useMemo(() => {
    const config = mergeWith(
      {},
      defaultTheme,
      nyanpasuConfig?.theme_setting || {},
      (objValue, srcValue) => {
        return !srcValue ? objValue : srcValue
      },
    )
    console.log('merged theme config: ', config)
    const mergedTheme = createMDYTheme(config)

    applyRootStyleVar(themeMode, mergedTheme)

    return mergedTheme
  }, [nyanpasuConfig?.theme_setting, themeMode])

  return { theme }
}

export const ThemeModeProvider = () => {
  const { nyanpasuConfig } = useNyanpasu()

  const setThemeMode = useSetAtom(themeModeAtom)

  const { setMode } = useColorScheme()

  useEffect(() => {
    if (nyanpasuConfig?.theme_mode === 'system') {
      appWindow.theme().then((m) => {
        if (m) {
          setThemeMode(m)
          setMode(m)
        }
      })
    } else {
      const chosenThemeMode = nyanpasuConfig?.theme_mode || 'light'
      setThemeMode(chosenThemeMode)
      setMode(chosenThemeMode)
    }

    const unlisten = appWindow.onThemeChanged((e) => {
      if (nyanpasuConfig?.theme_mode === 'system') {
        setThemeMode(e.payload)
        setMode(e.payload)
      }
    })

    return () => {
      unlisten.then((fn) => fn())
    }
  }, [nyanpasuConfig?.theme_mode, setMode, setThemeMode])

  return null
}
