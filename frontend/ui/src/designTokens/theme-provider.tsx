import React, { createContext, useContext, useEffect, useMemo } from 'react'
import { applyMD3Tokens, createMD3Tokens, MD3Tokens } from './md3-tokens'

interface ThemeContextValue {
  tokens: MD3Tokens
  mode: 'light' | 'dark'
  setMode: (mode: 'light' | 'dark') => void
  sourceColor: string
  setSourceColor: (color: string) => void
}

const ThemeContext = createContext<ThemeContextValue | null>(null)

interface MD3ThemeProviderProps {
  children: React.ReactNode
  sourceColor?: string
  fontFamily?: string
  mode?: 'light' | 'dark'
  onModeChange?: (mode: 'light' | 'dark') => void
  onSourceColorChange?: (color: string) => void
}

/**
 * MD3 Theme Provider that generates and applies CSS custom properties
 * Can coexist with MUI theme during migration
 */
export const MD3ThemeProvider: React.FC<MD3ThemeProviderProps> = ({
  children,
  sourceColor = '#6750A4', // Default Material You primary color
  fontFamily = 'system-ui, -apple-system, sans-serif',
  mode: initialMode = 'light',
  onModeChange,
  onSourceColorChange,
}) => {
  const [mode, setModeState] = React.useState<'light' | 'dark'>(initialMode)
  const [currentSourceColor, setCurrentSourceColor] =
    React.useState(sourceColor)

  // Generate tokens when source color changes
  const tokens = useMemo(() => {
    return createMD3Tokens(currentSourceColor, fontFamily)
  }, [currentSourceColor, fontFamily])

  // Apply tokens to document when they change
  useEffect(() => {
    applyMD3Tokens(tokens, mode)

    // Update document class for mode switching
    if (mode === 'dark') {
      document.documentElement.classList.add('dark')
      document.documentElement.setAttribute('data-theme', 'dark')
    } else {
      document.documentElement.classList.remove('dark')
      document.documentElement.setAttribute('data-theme', 'light')
    }
  }, [tokens, mode])

  const setMode = (newMode: 'light' | 'dark') => {
    setModeState(newMode)
    onModeChange?.(newMode)
  }

  const setSourceColor = (color: string) => {
    setCurrentSourceColor(color)
    onSourceColorChange?.(color)
  }

  const contextValue: ThemeContextValue = {
    tokens,
    mode,
    setMode,
    sourceColor: currentSourceColor,
    setSourceColor,
  }

  return (
    <ThemeContext.Provider value={contextValue}>
      {children}
    </ThemeContext.Provider>
  )
}

/**
 * Hook to access MD3 theme context
 */
export const useMD3Theme = (): ThemeContextValue => {
  const context = useContext(ThemeContext)
  if (!context) {
    throw new Error('useMD3Theme must be used within an MD3ThemeProvider')
  }
  return context
}

/**
 * Hook to get CSS custom property values
 */
export const useMD3Token = (tokenPath: string): string => {
  const { mode } = useMD3Theme()

  return useMemo(() => {
    const cssVar = `--md3-${tokenPath.replace(/\./g, '-')}`
    return `var(${cssVar})`
  }, [tokenPath, mode])
}

/**
 * Utility function to get CSS custom property value
 */
export const getMD3Token = (tokenPath: string): string => {
  const cssVar = `--md3-${tokenPath.replace(/\./g, '-')}`
  return `var(${cssVar})`
}

/**
 * Higher-order component to inject MD3 theme
 */
export const withMD3Theme = <P extends object>(
  Component: React.ComponentType<P>,
) => {
  const WrappedComponent = (props: P) => {
    const theme = useMD3Theme()
    return <Component {...props} md3Theme={theme} />
  }

  WrappedComponent.displayName = `withMD3Theme(${Component.displayName || Component.name})`
  return WrappedComponent
}
