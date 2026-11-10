import { createCssVariablesTheme } from 'shiki/core'
import { SYNTAX_CSS_VARIABLE_PREFIX } from '@nyanpasu/theme'

export const SYNTAX_THEME_NAME = 'nyanpasu'

/**
 * Colors tokens with the syntax CSS variables of the application theme, so
 * highlighted code follows the theme color and mode without highlighting again.
 */
export const syntaxTheme = createCssVariablesTheme({
  name: SYNTAX_THEME_NAME,
  variablePrefix: SYNTAX_CSS_VARIABLE_PREFIX,
})
