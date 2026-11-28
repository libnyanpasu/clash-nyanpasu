import { useState } from 'react'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { PageTitle } from '@/components/page-title'
import {
  useExperimentalThemeContext,
  type ThemeOptions,
} from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import {
  DEFAULT_COLOR,
  DEFAULT_CONTRAST_LEVEL,
  DEFAULT_THEME_SPEC_VERSION,
  DEFAULT_THEME_VARIANT,
  type ResolvedThemeMode,
} from '@nyanpasu/theme'
import { createFileRoute } from '@tanstack/react-router'
import { ThemeContrastSection } from './_modules/theme-contrast-section'
import {
  ThemePreviewSection,
  type ThemePreviewMode,
} from './_modules/theme-preview-section'
import { ThemeSaveDock } from './_modules/theme-save-dock'
import { ThemeSourceColorSection } from './_modules/theme-source-color-section'
import { ThemeStyleSection } from './_modules/theme-style-section'

export const Route = createFileRoute(
  '/(main)/main/settings/nyanpasu/interface/theme/',
)({
  component: RouteComponent,
  staticData: {
    crumb: { title: () => m.settings_user_interface_theme_color_label() },
  },
})

const DEFAULT_THEME_OPTIONS: ThemeOptions = {
  color: DEFAULT_COLOR,
  variant: DEFAULT_THEME_VARIANT,
  contrastLevel: DEFAULT_CONTRAST_LEVEL,
  specVersion: DEFAULT_THEME_SPEC_VERSION,
}

const isSameThemeOptions = (a: ThemeOptions, b: ThemeOptions) =>
  a.color.toLowerCase() === b.color.toLowerCase() &&
  a.variant === b.variant &&
  a.contrastLevel === b.contrastLevel &&
  a.specVersion === b.specVersion

function RouteComponent() {
  const { themeOptions, setThemeOptions, currentThemeMode } =
    useExperimentalThemeContext()

  // `null` mirrors the saved options until the user edits one.
  const [edits, setEdits] = useState<ThemeOptions | null>(null)
  const [previewMode, setPreviewMode] = useState<ThemePreviewMode>('both')
  const [applying, setApplying] = useState(false)

  const draft = edits ?? themeOptions
  const dirty = !isSameThemeOptions(draft, themeOptions)

  // Swatches follow the previewed mode; "both" follows the app.
  const swatchMode: ResolvedThemeMode =
    previewMode === 'both' ? currentThemeMode : previewMode

  // Edits made before the next render must build on each other.
  const update = (patch: Partial<ThemeOptions>) =>
    setEdits((current) => ({ ...(current ?? themeOptions), ...patch }))

  // Resolves whether the draft was saved.
  const handleApply = async () => {
    setApplying(true)

    try {
      await setThemeOptions(draft)
      setEdits(null)

      return true
    } catch (error) {
      message(`Update theme failed!\n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })

      return false
    } finally {
      setApplying(false)
    }
  }

  return (
    <>
      <PageTitle />

      <ReadOnlyScope permissions={['settings:write']}>
        <div className="space-y-4 px-4 pb-24">
          <ThemePreviewSection
            draft={draft}
            previewMode={previewMode}
            onPreviewModeChange={setPreviewMode}
          />

          <ThemeSourceColorSection
            draft={draft}
            swatchMode={swatchMode}
            onChange={(color) => update({ color })}
            onRestoreDefaults={() => setEdits(DEFAULT_THEME_OPTIONS)}
          />

          <ThemeStyleSection
            draft={draft}
            swatchMode={swatchMode}
            onChange={(variant) => update({ variant })}
          />

          <ThemeContrastSection
            draft={draft}
            onContrastLevelChange={(contrastLevel) => update({ contrastLevel })}
            onSpecVersionChange={(specVersion) => update({ specVersion })}
          />
        </div>

        <ThemeSaveDock
          dirty={dirty}
          applying={applying}
          onDiscard={() => setEdits(null)}
          onApply={handleApply}
        />
      </ReadOnlyScope>
    </>
  )
}
