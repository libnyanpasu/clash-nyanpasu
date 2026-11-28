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
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import { ThemeContrastSection } from './_modules/theme-contrast-section'
import { ThemePreviewSection } from './_modules/theme-preview-section'
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
  const [applying, setApplying] = useState(false)

  const draft = edits ?? themeOptions
  const dirty = !isSameThemeOptions(draft, themeOptions)

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
        <div className="flex flex-col gap-4 px-4 pb-24">
          <ThemePreviewSection draft={draft} />

          {/* The source color runs down one column beside the style and
              contrast sections once the page is wide enough. */}
          <div className="@container">
            <div
              data-slot="theme-settings-grid"
              className={cn(
                'grid grid-cols-1 gap-4',
                // Style heading, style card, a gap, contrast heading and
                // contrast card; the source color spans them all.
                '@3xl:grid-cols-2 @3xl:grid-rows-[auto_auto_1rem_auto_1fr] @3xl:gap-y-0',
              )}
            >
              <ThemeSourceColorSection
                className="@3xl:col-start-1 @3xl:row-start-1"
                draft={draft}
                swatchMode={currentThemeMode}
                onChange={(color) => update({ color })}
              />

              <ThemeStyleSection
                className="@3xl:col-start-2 @3xl:row-start-1"
                draft={draft}
                swatchMode={currentThemeMode}
                onChange={(variant) => update({ variant })}
              />

              <ThemeContrastSection
                className="@3xl:col-start-2 @3xl:row-start-4"
                draft={draft}
                onContrastLevelChange={(contrastLevel) =>
                  update({ contrastLevel })
                }
                onSpecVersionChange={(specVersion) => update({ specVersion })}
              />
            </div>
          </div>
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
