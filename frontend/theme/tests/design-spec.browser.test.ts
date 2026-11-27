import { describe, expect, it } from 'vitest'
import {
  createTheme,
  DEFAULT_COLOR,
  generateScopedThemeCssVars,
  getContrastRatio,
  getThemeScheme,
  THEME_COLOR_ROLES,
  ThemeMode,
} from '@nyanpasu/theme'
import {
  COLOR_ROLE_GROUPS,
  CONTRAST_PAIRS,
  SURFACE_LADDER,
  THEME_PRESET_SEEDS,
} from '@nyanpasu/theme/spec'

describe('M3E / A17 design-as-code spec', () => {
  it('documents every single one of the 49 M3E color roles without gaps or duplicates', () => {
    const documentedRoles = COLOR_ROLE_GROUPS.flatMap((group) =>
      group.roles.map((entry) => entry.role),
    )

    expect(new Set(documentedRoles).size).toBe(documentedRoles.length)
    expect([...documentedRoles].sort()).toEqual([...THEME_COLOR_ROLES].sort())
  })

  it('references valid M3E color roles across the 12-tier A17 surface ladder', () => {
    const roleSet = new Set<string>(THEME_COLOR_ROLES)

    for (const tier of SURFACE_LADDER) {
      expect(roleSet.has(tier.lightRole)).toBe(true)
      expect(roleSet.has(tier.darkRole)).toBe(true)
      expect(roleSet.has(tier.foregroundRole)).toBe(true)
    }
  })

  it('maintains WCAG AA contrast (>= 4.5:1) for canonical X/on-X pairs across preset seeds', () => {
    for (const preset of THEME_PRESET_SEEDS) {
      for (const variant of ['tonalSpot', 'expressive'] as const) {
        const { palette } = createTheme(preset.hex, { variant })

        for (const mode of [ThemeMode.LIGHT, ThemeMode.DARK] as const) {
          const scheme = getThemeScheme(palette, mode)

          for (const pair of CONTRAST_PAIRS) {
            const ratio = getContrastRatio(
              scheme[pair.background],
              scheme[pair.foreground],
            )
            expect(
              ratio,
              `${preset.name} (${variant}, ${mode}) ${pair.label} ratio=${ratio.toFixed(2)}`,
            ).toBeGreaterThanOrEqual(pair.minRatio)
          }
        }
      }
    }
  })

  it('emits scoped CSS variables for split light/dark previews', () => {
    const { palette } = createTheme(DEFAULT_COLOR)
    const scopedDark = generateScopedThemeCssVars(
      palette,
      '.gallery-theme-dark',
      ThemeMode.DARK,
    )

    expect(scopedDark.startsWith('.gallery-theme-dark{')).toBe(true)
    expect(scopedDark).toContain('--color-md-surface-container-lowest:')
    expect(scopedDark).toContain('--syntax-token-keyword:')
  })
})
