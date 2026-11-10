import * as monaco from 'monaco-editor'
import { useEffect } from 'react'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { createMonacoTheme, monacoThemeName } from './monaco-theme'

/**
 * Keeps Monaco drawn in the application theme, following its color and mode,
 * and returns the name to pass to the editor.
 */
export function useMonacoTheme() {
  const { themePalette, currentThemeMode } = useExperimentalThemeContext()
  const name = monacoThemeName(currentThemeMode)

  // The editor asks for the theme by name before this effect has defined it
  // when the mode changes, so the theme is applied again once it exists.
  useEffect(() => {
    monaco.editor.defineTheme(
      name,
      createMonacoTheme(themePalette, currentThemeMode),
    )
    monaco.editor.setTheme(name)
  }, [name, themePalette, currentThemeMode])

  return name
}
