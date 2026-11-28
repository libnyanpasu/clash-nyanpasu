import CheckRounded from '~icons/material-symbols/check-rounded'
import DarkModeRounded from '~icons/material-symbols/dark-mode-rounded'
import LightModeRounded from '~icons/material-symbols/light-mode-rounded'
import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded'
import SplitscreenLeftRounded from '~icons/material-symbols/splitscreen-left-rounded'
import UndoRounded from '~icons/material-symbols/undo-rounded'
import { AnimatePresence, motion } from 'motion/react'
import {
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
import {
  createTheme,
  DEFAULT_COLOR,
  DEFAULT_CONTRAST_LEVEL,
  DEFAULT_THEME_SPEC_VERSION,
  DEFAULT_THEME_VARIANT,
  getContrastRatio,
  getThemeScheme,
  hexFromArgb,
  ThemeMode,
  type ResolvedThemeMode,
  type ThemeColorRole,
  type ThemeScheme,
  type ThemeSpecVersion,
  type ThemeVariant,
} from '@nyanpasu/theme'
import { THEME_PRESET_SEEDS, THEME_VARIANT_SPECS } from '@nyanpasu/theme/spec'
import {
  ActionSwapText,
  Button,
  Card,
  ColorSchemeSwatch,
  ColorSchemeSwatchGroup,
  GlassControlGroup,
  HctColorPicker,
  HctColorPickerHexField,
  HctColorPickerSlider,
  HctColorPickerSliders,
  SegmentedButton,
  SegmentedButtonItem,
  ThemeMockup,
} from '@nyanpasu/ui'
import { cn } from '@nyanpasu/utils'
import { PageHeader } from '../studio/stage'
import { useThemeStudio } from '../studio/theme-studio-context'

type PreviewMode = 'light' | 'dark' | 'split'

type ThemeDraft = {
  seedColor: string
  variant: ThemeVariant
  contrastLevel: number
  specVersion: ThemeSpecVersion
}

const DEFAULT_DRAFT: ThemeDraft = {
  seedColor: DEFAULT_COLOR,
  variant: DEFAULT_THEME_VARIANT,
  contrastLevel: DEFAULT_CONTRAST_LEVEL,
  specVersion: DEFAULT_THEME_SPEC_VERSION,
}

const CONTRAST_OPTIONS = [
  { value: -1, label: 'Reduced' },
  { value: 0, label: 'Standard' },
  { value: 0.5, label: 'Medium' },
  { value: 1, label: 'High' },
] as const

const SPEC_OPTIONS: readonly {
  value: ThemeSpecVersion
  label: string
  summary: string
}[] = [
  {
    value: '2025',
    label: 'M3 Expressive · 2025',
    summary:
      'Android 16+ tone curves: brighter containers, softer dark surfaces and the expressive fixed roles.',
  },
  {
    value: '2021',
    label: 'Material You · 2021',
    summary:
      'The original Android 12 dynamic color tones, for a flatter and more classic look.',
  },
]

const ROLE_FAMILIES: readonly {
  name: string
  roles: readonly [
    ThemeColorRole,
    ThemeColorRole,
    ThemeColorRole,
    ThemeColorRole,
  ]
}[] = [
  {
    name: 'Primary',
    roles: ['primary', 'onPrimary', 'primaryContainer', 'onPrimaryContainer'],
  },
  {
    name: 'Secondary',
    roles: [
      'secondary',
      'onSecondary',
      'secondaryContainer',
      'onSecondaryContainer',
    ],
  },
  {
    name: 'Tertiary',
    roles: [
      'tertiary',
      'onTertiary',
      'tertiaryContainer',
      'onTertiaryContainer',
    ],
  },
  {
    name: 'Error',
    roles: ['error', 'onError', 'errorContainer', 'onErrorContainer'],
  },
]

const SURFACE_ROLES: readonly ThemeColorRole[] = [
  'surfaceContainerLowest',
  'surfaceContainerLow',
  'surfaceContainer',
  'surfaceContainerHigh',
  'surfaceContainerHighest',
  'surfaceDim',
  'surfaceBright',
]

const CONTRAST_PAIRS: readonly {
  label: string
  foreground: ThemeColorRole
  background: ThemeColorRole
}[] = [
  { label: 'Body text', foreground: 'onSurface', background: 'surface' },
  {
    label: 'Muted text',
    foreground: 'onSurfaceVariant',
    background: 'surfaceContainerHighest',
  },
  { label: 'Filled button', foreground: 'onPrimary', background: 'primary' },
  {
    label: 'Tonal button',
    foreground: 'onSecondaryContainer',
    background: 'secondaryContainer',
  },
]

const roleHex = (scheme: ThemeScheme, role: ThemeColorRole) =>
  hexFromArgb(scheme[role])

const swatchColors = (scheme: ThemeScheme) => ({
  primary: roleHex(scheme, 'primary'),
  secondary: roleHex(scheme, 'secondaryContainer'),
  tertiary: roleHex(scheme, 'tertiary'),
  surface: roleHex(scheme, 'surfaceContainer'),
})

const toKebab = (role: string) =>
  role.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)

function StudioSection({
  title,
  description,
  action,
  className,
  children,
}: {
  title: string
  description: string
  action?: ReactNode
  className?: string
  children: ReactNode
}) {
  return (
    <Card
      data-slot="palette-studio-section"
      className={cn('flex flex-col gap-4 p-5', className)}
    >
      <header className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <h2 className="text-on-surface text-base font-semibold">{title}</h2>
          <p className="text-on-surface-variant text-xs leading-5">
            {description}
          </p>
        </div>

        {action}
      </header>

      {children}
    </Card>
  )
}

/** How long the dock confirms an applied theme before it leaves. */
const APPLIED_NOTICE_MS = 2000

// The settings page's save dock. It sits above the gallery's own floating
// studio dock.
function PaletteSaveDock({
  dirty,
  onDiscard,
  onApply,
}: {
  dirty: boolean
  onDiscard: () => void
  onApply: () => void
}) {
  const [applied, setApplied] = useState(false)

  useEffect(() => {
    if (!applied) {
      return
    }

    const timer = setTimeout(() => setApplied(false), APPLIED_NOTICE_MS)

    return () => clearTimeout(timer)
  }, [applied])

  const status = dirty ? 'Unsaved changes' : applied ? 'Theme applied' : null

  return (
    <AnimatePresence>
      {status !== null && (
        <motion.div
          data-slot="palette-studio-save-dock"
          className="fixed right-4 bottom-20 z-40"
          initial={{ opacity: 0, y: 16 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: 16 }}
          transition={{ duration: 0.2, ease: 'easeOut' }}
        >
          <GlassControlGroup role="status" className="gap-1.5 pl-4">
            <ActionSwapText
              className="text-on-surface text-sm font-medium whitespace-nowrap"
              value={status}
            />

            <Button
              variant="glass"
              className="gap-1.5"
              disabled={!dirty}
              onClick={() => {
                setApplied(false)
                onDiscard()
              }}
            >
              <UndoRounded className="size-4" />
              Discard
            </Button>

            <Button
              variant="flat"
              className="gap-1.5"
              disabled={!dirty}
              onClick={() => {
                onApply()
                setApplied(true)
              }}
            >
              <CheckRounded className="size-4" />
              Apply
            </Button>
          </GlassControlGroup>
        </motion.div>
      )}
    </AnimatePresence>
  )
}

function PalettePreviewSection({
  draft,
  previewMode,
  onPreviewModeChange,
}: {
  draft: ThemeDraft
  previewMode: PreviewMode
  onPreviewModeChange: (mode: PreviewMode) => void
}) {
  const deferredDraft = useDeferredValue(draft)

  const { seedColor, variant, contrastLevel, specVersion } = deferredDraft

  const palette = useMemo(
    () =>
      createTheme(seedColor, { variant, contrastLevel, specVersion }).palette,
    [seedColor, variant, contrastLevel, specVersion],
  )

  const modes: ResolvedThemeMode[] =
    previewMode === 'split'
      ? [ThemeMode.LIGHT, ThemeMode.DARK]
      : [previewMode === 'dark' ? ThemeMode.DARK : ThemeMode.LIGHT]

  return (
    <Card
      data-slot="palette-studio-preview"
      className="flex flex-col gap-4 p-4 sm:p-5"
    >
      <div className="flex justify-center">
        <SegmentedButton
          aria-label="Preview mode"
          className="w-auto"
          value={previewMode}
          onValueChange={(next) => {
            if (next) onPreviewModeChange(next as PreviewMode)
          }}
        >
          <SegmentedButtonItem value="light" className="gap-1.5">
            <LightModeRounded className="size-4" />
            Light
          </SegmentedButtonItem>

          <SegmentedButtonItem value="dark" className="gap-1.5">
            <DarkModeRounded className="size-4" />
            Dark
          </SegmentedButtonItem>

          <SegmentedButtonItem value="split" className="gap-1.5">
            <SplitscreenLeftRounded className="size-4" />
            Both
          </SegmentedButtonItem>
        </SegmentedButton>
      </div>

      <div
        data-slot="palette-studio-preview-frames"
        className={cn(
          'grid justify-center gap-4',
          previewMode === 'split'
            ? 'sm:grid-cols-[repeat(2,minmax(0,20rem))]'
            : 'grid-cols-[minmax(0,20rem)]',
        )}
      >
        {modes.map((mode) => (
          <ThemeMockup
            key={mode}
            palette={palette}
            mode={mode}
            aria-label={`Clash Nyanpasu preview (${mode})`}
          />
        ))}
      </div>
    </Card>
  )
}

export function PaletteStudioPage() {
  const studio = useThemeStudio()

  const applied: ThemeDraft = {
    seedColor: studio.seedColor,
    variant: studio.variant,
    contrastLevel: studio.contrastLevel,
    specVersion: studio.specVersion,
  }

  // `null` mirrors the applied theme, so changes made elsewhere (the
  // floating dock) show up until the user starts editing here.
  const [edits, setEdits] = useState<ThemeDraft | null>(null)
  const [previewMode, setPreviewMode] = useState<PreviewMode>('split')

  const draft = edits ?? applied
  const deferredDraft = useDeferredValue(draft)

  const dirty =
    draft.seedColor.toLowerCase() !== applied.seedColor.toLowerCase() ||
    draft.variant !== applied.variant ||
    draft.contrastLevel !== applied.contrastLevel ||
    draft.specVersion !== applied.specVersion

  // Swatches follow the mode being previewed; "Both" uses the gallery mode.
  const swatchMode: ResolvedThemeMode =
    previewMode === 'split'
      ? studio.resolvedMode
      : previewMode === 'dark'
        ? ThemeMode.DARK
        : ThemeMode.LIGHT

  const { seedColor, variant, contrastLevel, specVersion } = deferredDraft

  const scheme = useMemo(
    () =>
      getThemeScheme(
        createTheme(seedColor, { variant, contrastLevel, specVersion }).palette,
        swatchMode,
      ),
    [seedColor, variant, contrastLevel, specVersion, swatchMode],
  )

  const presetSwatches = useMemo(
    () =>
      THEME_PRESET_SEEDS.map((preset) => ({
        ...preset,
        colors: swatchColors(
          getThemeScheme(
            createTheme(preset.hex, { variant, contrastLevel, specVersion })
              .palette,
            swatchMode,
          ),
        ),
      })),
    [variant, contrastLevel, specVersion, swatchMode],
  )

  const variantSwatches = useMemo(
    () =>
      THEME_VARIANT_SPECS.map((spec) => ({
        ...spec,
        colors: swatchColors(
          getThemeScheme(
            createTheme(seedColor, {
              variant: spec.id,
              contrastLevel,
              specVersion,
            }).palette,
            swatchMode,
          ),
        ),
      })),
    [seedColor, contrastLevel, specVersion, swatchMode],
  )

  const activeVariant = THEME_VARIANT_SPECS.find(
    (spec) => spec.id === draft.variant,
  )
  const activeSpec = SPEC_OPTIONS.find(
    (spec) => spec.value === draft.specVersion,
  )
  const presetValue =
    THEME_PRESET_SEEDS.find(
      (preset) => preset.hex.toLowerCase() === draft.seedColor.toLowerCase(),
    )?.hex ?? ''

  const update = (patch: Partial<ThemeDraft>) =>
    setEdits({ ...draft, ...patch })

  const handleApply = () => {
    studio.setSeedColor(draft.seedColor)
    studio.setVariant(draft.variant)
    studio.setContrastLevel(draft.contrastLevel)
    studio.setSpecVersion(draft.specVersion)
    setEdits(null)
  }

  return (
    <div
      className="flex flex-col gap-6"
      data-slot="gallery-palette-studio-page"
    >
      <PageHeader
        eyebrow="Patterns · Palette Studio"
        title="Palette Studio"
        description="A settings-page prototype of the full Theme Studio: pick a source color, a DynamicScheme style, a contrast level and the color spec, and check the result on a live Clash Nyanpasu preview in light and dark before applying it."
        badges={['createTheme', 'DynamicScheme', `${swatchMode} swatches`]}
      />

      <PalettePreviewSection
        draft={draft}
        previewMode={previewMode}
        onPreviewModeChange={setPreviewMode}
      />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <StudioSection
          title="Source color"
          description="Every role is derived from one seed. Pick a preset or dial in any HCT color."
          className="lg:row-span-2"
          action={
            <Button
              variant="basic"
              icon
              className="size-9 shrink-0"
              aria-label="Restore defaults"
              title="Restore defaults"
              onClick={() => setEdits(DEFAULT_DRAFT)}
            >
              <RestartAltRounded className="size-5" />
            </Button>
          }
        >
          <ColorSchemeSwatchGroup
            aria-label="Preset source colors"
            value={presetValue}
            onValueChange={(hex) => update({ seedColor: hex })}
          >
            {presetSwatches.map((preset) => (
              <ColorSchemeSwatch
                key={preset.hex}
                value={preset.hex}
                label={preset.name}
                colors={preset.colors}
              />
            ))}
          </ColorSchemeSwatchGroup>

          <HctColorPicker
            className="gap-3"
            value={draft.seedColor}
            onValueChange={(hex) => update({ seedColor: hex })}
          >
            <HctColorPickerHexField label="Source color" />

            <HctColorPickerSliders>
              <HctColorPickerSlider channel="hue" label="Hue" />
              <HctColorPickerSlider channel="chroma" label="Chroma" />
              <HctColorPickerSlider channel="tone" label="Tone" />
            </HctColorPickerSliders>
          </HctColorPicker>
        </StudioSection>

        <StudioSection
          title="Scheme style"
          description="How the seed spreads across primary, secondary, tertiary and the neutral surfaces."
        >
          <ColorSchemeSwatchGroup
            aria-label="Scheme style"
            value={draft.variant}
            onValueChange={(variant) =>
              update({ variant: variant as ThemeVariant })
            }
          >
            {variantSwatches.map((spec) => (
              <ColorSchemeSwatch
                key={spec.id}
                value={spec.id}
                label={spec.label}
                colors={spec.colors}
              />
            ))}
          </ColorSchemeSwatchGroup>

          {activeVariant && (
            <div className="bg-surface-container dark:bg-surface-container-high rounded-large flex flex-col gap-0.5 px-4 py-3">
              <span className="text-on-surface text-sm font-medium">
                {activeVariant.label}
              </span>
              <span className="text-on-surface-variant text-xs leading-5">
                {activeVariant.summary}
              </span>
            </div>
          )}
        </StudioSection>

        <StudioSection
          title="Contrast & spec"
          description="Raise contrast for legibility, or switch between the 2025 Expressive and 2021 Material You tone curves."
        >
          <div className="flex flex-col gap-2">
            <span className="text-on-surface-variant text-xs font-medium">
              Contrast
            </span>

            <SegmentedButton
              aria-label="Contrast level"
              className="w-full"
              value={String(draft.contrastLevel)}
              onValueChange={(next) => {
                if (next) update({ contrastLevel: Number(next) })
              }}
            >
              {CONTRAST_OPTIONS.map((option) => (
                <SegmentedButtonItem
                  key={option.value}
                  value={String(option.value)}
                  className="flex-1"
                >
                  {option.label}
                </SegmentedButtonItem>
              ))}
            </SegmentedButton>
          </div>

          <div className="flex flex-col gap-2">
            <span className="text-on-surface-variant text-xs font-medium">
              Color spec
            </span>

            <SegmentedButton
              aria-label="Color spec"
              className="w-full"
              value={draft.specVersion}
              onValueChange={(next) => {
                if (next) update({ specVersion: next as ThemeSpecVersion })
              }}
            >
              {SPEC_OPTIONS.map((option) => (
                <SegmentedButtonItem
                  key={option.value}
                  value={option.value}
                  className="flex-1"
                >
                  {option.label}
                </SegmentedButtonItem>
              ))}
            </SegmentedButton>

            {activeSpec && (
              <p className="text-on-surface-variant text-xs leading-5">
                {activeSpec.summary}
              </p>
            )}
          </div>

          <div
            data-slot="palette-studio-contrast-readout"
            className="grid grid-cols-2 gap-2"
          >
            {CONTRAST_PAIRS.map((pair) => {
              const ratio = getContrastRatio(
                scheme[pair.foreground],
                scheme[pair.background],
              )

              return (
                <div
                  key={pair.label}
                  className="rounded-medium flex items-center justify-between gap-2 px-3 py-2"
                  style={{
                    backgroundColor: roleHex(scheme, pair.background),
                    color: roleHex(scheme, pair.foreground),
                  }}
                >
                  <span className="truncate text-xs font-medium">
                    {pair.label}
                  </span>
                  <span className="font-mono text-xs tabular-nums">
                    {ratio.toFixed(1)}:1
                  </span>
                </div>
              )
            })}
          </div>
        </StudioSection>
      </div>

      <StudioSection
        title="Color roles"
        description={`The key roles this draft produces in ${swatchMode} mode, paired the way components use them.`}
      >
        <div
          data-slot="palette-studio-role-families"
          className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4"
        >
          {ROLE_FAMILIES.map((family) => {
            const [base, onBase, container, onContainer] = family.roles

            return (
              <div
                key={family.name}
                className="rounded-large flex flex-col overflow-hidden"
              >
                <div
                  className="flex h-20 flex-col justify-between p-3"
                  style={{
                    backgroundColor: roleHex(scheme, base),
                    color: roleHex(scheme, onBase),
                  }}
                >
                  <span className="text-sm font-semibold">{family.name}</span>
                  <span className="font-mono text-xs">
                    {roleHex(scheme, base)}
                  </span>
                </div>

                <div
                  className="flex h-16 flex-col justify-between p-3"
                  style={{
                    backgroundColor: roleHex(scheme, container),
                    color: roleHex(scheme, onContainer),
                  }}
                >
                  <span className="text-xs font-medium">
                    {family.name} container
                  </span>
                  <span className="font-mono text-xs">
                    {roleHex(scheme, container)}
                  </span>
                </div>
              </div>
            )
          })}
        </div>

        <div
          data-slot="palette-studio-surface-strip"
          className="rounded-large grid grid-cols-7 overflow-hidden"
        >
          {SURFACE_ROLES.map((role) => (
            <div
              key={role}
              title={`${toKebab(role)} ${roleHex(scheme, role)}`}
              className="flex h-16 flex-col justify-end p-2"
              style={{
                backgroundColor: roleHex(scheme, role),
                color: roleHex(scheme, 'onSurfaceVariant'),
              }}
            >
              <span className="truncate font-mono text-[10px]">
                {toKebab(role).replace('surface-container', 'sc')}
              </span>
            </div>
          ))}
        </div>
      </StudioSection>

      <PaletteSaveDock
        dirty={dirty}
        onDiscard={() => setEdits(null)}
        onApply={handleApply}
      />
    </div>
  )
}
