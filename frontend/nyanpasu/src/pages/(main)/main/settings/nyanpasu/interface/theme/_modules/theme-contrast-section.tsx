import { cva } from 'class-variance-authority'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import type { ThemeOptions } from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import type { ThemeSpecVersion } from '@nyanpasu/theme'
import { ThemeSection } from './theme-section'

const CONTRAST_OPTIONS = [
  { value: -1, label: m.settings_user_interface_theme_contrast_reduced },
  { value: 0, label: m.settings_user_interface_theme_contrast_standard },
  { value: 0.5, label: m.settings_user_interface_theme_contrast_medium },
  { value: 1, label: m.settings_user_interface_theme_contrast_high },
] as const

const SPEC_OPTIONS: readonly {
  value: ThemeSpecVersion
  label: () => string
  summary: () => string
}[] = [
  {
    value: '2025',
    label: m.settings_user_interface_theme_spec_2025,
    summary: m.settings_user_interface_theme_spec_2025_summary,
  },
  {
    value: '2021',
    label: m.settings_user_interface_theme_spec_2021,
    summary: m.settings_user_interface_theme_spec_2021_summary,
  },
]

const fieldLabelVariants = cva('text-on-surface-variant text-xs font-medium')

export function ThemeContrastSection({
  draft,
  onContrastLevelChange,
  onSpecVersionChange,
}: {
  draft: ThemeOptions
  onContrastLevelChange: (contrastLevel: number) => void
  onSpecVersionChange: (specVersion: ThemeSpecVersion) => void
}) {
  const spec = SPEC_OPTIONS.find((option) => option.value === draft.specVersion)

  return (
    <ThemeSection
      data-slot="theme-contrast-section"
      title={m.settings_user_interface_theme_contrast_title()}
      description={m.settings_user_interface_theme_contrast_description()}
    >
      <div data-slot="theme-contrast-level" className="flex flex-col gap-2">
        <span className={fieldLabelVariants()}>
          {m.settings_user_interface_theme_contrast_label()}
        </span>

        <SegmentedButton
          aria-label={m.settings_user_interface_theme_contrast_label()}
          className="w-full"
          value={String(draft.contrastLevel)}
          onValueChange={(next) => {
            if (next) {
              onContrastLevelChange(Number(next))
            }
          }}
        >
          {CONTRAST_OPTIONS.map((option) => (
            <SegmentedButtonItem
              key={option.value}
              value={String(option.value)}
              className="flex-1"
            >
              {option.label()}
            </SegmentedButtonItem>
          ))}
        </SegmentedButton>
      </div>

      <div data-slot="theme-spec-version" className="flex flex-col gap-2">
        <span className={fieldLabelVariants()}>
          {m.settings_user_interface_theme_spec_label()}
        </span>

        <SegmentedButton
          aria-label={m.settings_user_interface_theme_spec_label()}
          className="w-full"
          value={draft.specVersion}
          onValueChange={(next) => {
            if (next) {
              onSpecVersionChange(next as ThemeSpecVersion)
            }
          }}
        >
          {SPEC_OPTIONS.map((option) => (
            <SegmentedButtonItem
              key={option.value}
              value={option.value}
              className="flex-1"
            >
              {option.label()}
            </SegmentedButtonItem>
          ))}
        </SegmentedButton>

        {spec && (
          <p className="text-on-surface-variant text-xs leading-5">
            {spec.summary()}
          </p>
        )}
      </div>
    </ThemeSection>
  )
}
