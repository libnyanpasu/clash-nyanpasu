import type { editor } from 'monaco-editor'
import {
  getSyntaxColors,
  getThemeScheme,
  hexFromArgb,
  ThemeMode,
  type ResolvedThemeMode,
  type Theme,
} from '@nyanpasu/theme'

export const monacoThemeName = (mode: ResolvedThemeMode) => `nyanpasu-${mode}`

const withAlpha = (color: string, alpha: number) =>
  `${color}${Math.round(alpha * 255)
    .toString(16)
    .padStart(2, '0')}`

/**
 * The Monaco theme of the application theme: the editor's surfaces take the
 * Material roles, and its tokens the syntax colors every highlighter shares.
 */
export function createMonacoTheme(
  theme: Theme,
  mode: ResolvedThemeMode,
): editor.IStandaloneThemeData {
  const scheme = getThemeScheme(theme, mode)
  const syntax = getSyntaxColors(theme, mode)

  const surface = hexFromArgb(scheme.surface)
  const surfaceVariant = hexFromArgb(scheme.surfaceVariant)
  const onSurface = hexFromArgb(scheme.onSurface)
  const onSurfaceVariant = hexFromArgb(scheme.onSurfaceVariant)
  const primary = hexFromArgb(scheme.primary)
  const secondaryContainer = hexFromArgb(scheme.secondaryContainer)
  const onSecondaryContainer = hexFromArgb(scheme.onSecondaryContainer)
  const tertiary = hexFromArgb(scheme.tertiary)
  const outline = hexFromArgb(scheme.outline)
  const outlineVariant = hexFromArgb(scheme.outlineVariant)

  // Token rules take a color without the leading `#`.
  const rule = (
    token: string,
    color: string,
    fontStyle?: string,
  ): editor.ITokenThemeRule => ({
    token,
    foreground: color.slice(1),
    fontStyle,
  })

  return {
    base: mode === ThemeMode.DARK ? 'vs-dark' : 'vs',
    // The base themes color some languages' tokens on their own, such as
    // YAML strings; without them every token takes a color from this list.
    inherit: false,
    rules: [
      rule('', syntax.foreground),
      rule('comment', syntax.comment, 'italic'),
      rule('keyword', syntax.keyword),
      rule('tag', syntax.keyword),
      rule('type', syntax.function),
      rule('namespace', syntax.function),
      rule('annotation', syntax.function),
      rule('attribute.name', syntax.function),
      rule('attribute.value', syntax.stringExpression),
      rule('string', syntax.stringExpression),
      rule('regexp', syntax.stringExpression),
      rule('string.escape', syntax.constant),
      rule('number', syntax.constant),
      rule('constant', syntax.constant),
      rule('variable.predefined', syntax.constant),
      rule('attribute.value.number', syntax.constant),
      rule('attribute.value.unit', syntax.constant),
      rule('attribute.value.hex', syntax.constant),
      rule('variable.parameter', syntax.parameter),
      rule('delimiter', syntax.punctuation),
      rule('operators', syntax.punctuation),
      // YAML and JSON as the shared syntax theme colors them elsewhere: keys
      // are keywords, and `true`, `false` and `null` are constants.
      rule('type.yaml', syntax.keyword),
      rule('keyword.yaml', syntax.constant),
      rule('string.key.json', syntax.keyword),
      rule('keyword.json', syntax.constant),
    ],
    colors: {
      focusBorder: primary,
      'editor.background': surface,
      'editor.foreground': onSurface,
      'editorGutter.background': surface,
      'editorCursor.foreground': primary,
      'editorLineNumber.foreground': outline,
      'editorLineNumber.activeForeground': onSurface,
      'editor.lineHighlightBackground': withAlpha(surfaceVariant, 0.4),
      'editor.lineHighlightBorder': withAlpha(surfaceVariant, 0),
      'editor.selectionBackground': withAlpha(primary, 0.3),
      'editor.inactiveSelectionBackground': withAlpha(primary, 0.15),
      'editor.selectionHighlightBackground': withAlpha(primary, 0.15),
      'editor.wordHighlightBackground': withAlpha(primary, 0.15),
      'editor.findMatchBackground': withAlpha(tertiary, 0.5),
      'editor.findMatchHighlightBackground': withAlpha(tertiary, 0.25),
      'editorBracketMatch.background': withAlpha(primary, 0.2),
      'editorBracketMatch.border': withAlpha(primary, 0.5),
      'editorIndentGuide.background1': withAlpha(outlineVariant, 0.5),
      'editorIndentGuide.activeBackground1': outline,
      'editorWhitespace.foreground': outlineVariant,
      'editorLink.activeForeground': syntax.link,
      'editorError.foreground': hexFromArgb(scheme.error),
      'editorWarning.foreground': syntax.changed,
      'editorWidget.background': surfaceVariant,
      'editorWidget.foreground': onSurfaceVariant,
      'editorWidget.border': outlineVariant,
      'editorHoverWidget.background': surfaceVariant,
      'editorHoverWidget.border': outlineVariant,
      'editorSuggestWidget.background': surfaceVariant,
      'editorSuggestWidget.border': outlineVariant,
      'editorSuggestWidget.foreground': onSurfaceVariant,
      'editorSuggestWidget.selectedBackground': secondaryContainer,
      'editorSuggestWidget.selectedForeground': onSecondaryContainer,
      'editorSuggestWidget.highlightForeground': primary,
      'input.background': surface,
      'input.foreground': onSurface,
      'input.border': outlineVariant,
      'inputOption.activeBorder': primary,
      'inputOption.activeBackground': withAlpha(primary, 0.25),
      'inputOption.activeForeground': onSurface,
      'list.hoverBackground': withAlpha(onSurface, 0.08),
      'list.activeSelectionBackground': secondaryContainer,
      'list.activeSelectionForeground': onSecondaryContainer,
      'list.focusBackground': secondaryContainer,
      'list.focusForeground': onSecondaryContainer,
      'list.highlightForeground': primary,
      'quickInput.background': surfaceVariant,
      'quickInput.foreground': onSurfaceVariant,
      'menu.background': surfaceVariant,
      'menu.foreground': onSurfaceVariant,
      'menu.selectionBackground': secondaryContainer,
      'menu.selectionForeground': onSecondaryContainer,
      'menu.separatorBackground': outlineVariant,
      'scrollbar.shadow': withAlpha(surface, 0),
      'scrollbarSlider.background': withAlpha(onSurface, 0.2),
      'scrollbarSlider.hoverBackground': withAlpha(onSurface, 0.35),
      'scrollbarSlider.activeBackground': withAlpha(onSurface, 0.5),
    },
  }
}
