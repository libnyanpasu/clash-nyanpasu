import AutoAwesomeMotionRounded from '~icons/material-symbols/auto-awesome-motion-rounded'
import BlurOffRounded from '~icons/material-symbols/blur-off-rounded'
import DarkModeRounded from '~icons/material-symbols/dark-mode-rounded'
import DesktopWindowsRounded from '~icons/material-symbols/desktop-windows-rounded'
import GridViewRounded from '~icons/material-symbols/grid-view-rounded'
import LightModeRounded from '~icons/material-symbols/light-mode-rounded'
import PaletteRounded from '~icons/material-symbols/palette-rounded'
import PhoneAndroidRounded from '~icons/material-symbols/phone-android-rounded'
import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded'
import SplitscreenLeftRounded from '~icons/material-symbols/splitscreen-left-rounded'
import TabletRounded from '~icons/material-symbols/tablet-rounded'
import WidthFullRounded from '~icons/material-symbols/width-full-rounded'
import type { ThemeSpecVersion, ThemeVariant } from '@nyanpasu/theme'
import { THEME_PRESET_SEEDS, THEME_VARIANT_SPECS } from '@nyanpasu/theme/spec'
import {
  Button,
  GlassControlGroup,
  HctColorPicker,
  HctColorPickerHexField,
  HctColorPickerPreset,
  HctColorPickerPresets,
  HctColorPickerSlider,
  HctColorPickerSliders,
  Popover,
  PopoverContent,
  PopoverTrigger,
  SegmentedButton,
  SegmentedButtonItem,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Slider,
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@nyanpasu/ui'
import { cn } from '@nyanpasu/utils'
import {
  useThemeStudio,
  type GalleryViewMode,
  type GalleryViewportPreset,
} from './theme-studio-context'

export function FloatingStudioDock() {
  const {
    seedColor,
    variant,
    contrastLevel,
    specVersion,
    mode,
    viewport,
    reducedMotion,
    reducedTransparency,
    xraySlots,
    setSeedColor,
    setVariant,
    setContrastLevel,
    setSpecVersion,
    setMode,
    setViewport,
    setReducedMotion,
    setReducedTransparency,
    setXraySlots,
    resetTheme,
  } = useThemeStudio()

  const activeVariantMeta = THEME_VARIANT_SPECS.find(
    (item) => item.id === variant,
  )

  return (
    <div
      data-slot="gallery-floating-dock"
      className="pointer-events-none fixed inset-x-0 bottom-3 z-40 flex justify-center px-3"
    >
      <div className="pointer-events-auto flex max-w-full flex-wrap items-center justify-center gap-2">
        <GlassControlGroup className="gap-1.5 px-1.5">
          <Popover>
            <Tooltip>
              <TooltipTrigger asChild>
                <PopoverTrigger asChild>
                  <Button
                    variant="glass"
                    className="gap-2 px-3 text-xs font-medium"
                    aria-label="Open Theme Studio"
                  >
                    <span
                      className="border-outline-variant/60 size-3.5 shrink-0 rounded-full border"
                      style={{ backgroundColor: seedColor }}
                    />
                    <PaletteRounded className="size-4" />
                    <span className="hidden sm:inline">
                      {activeVariantMeta?.label ?? variant}
                    </span>
                  </Button>
                </PopoverTrigger>
              </TooltipTrigger>
              <TooltipContent>
                Theme Studio (HCT, Scheme, Contrast)
              </TooltipContent>
            </Tooltip>

            <PopoverContent
              side="top"
              align="start"
              sideOffset={14}
              className="w-88 max-w-[calc(100vw-2rem)] p-4"
            >
              <div className="flex flex-col gap-4">
                <div className="flex items-center justify-between gap-2">
                  <div>
                    <div className="text-on-surface text-sm font-semibold">
                      Theme Studio
                    </div>
                    <div className="text-on-surface-variant text-xs">
                      Live 2025 M3E DynamicScheme workbench
                    </div>
                  </div>

                  <Button
                    variant="basic"
                    icon
                    className="size-8"
                    aria-label="Reset theme studio"
                    onClick={resetTheme}
                  >
                    <RestartAltRounded className="size-4" />
                  </Button>
                </div>

                <HctColorPicker value={seedColor} onValueChange={setSeedColor}>
                  <HctColorPickerPresets className="flex flex-wrap gap-2">
                    {THEME_PRESET_SEEDS.map((preset) => (
                      <HctColorPickerPreset
                        key={preset.hex}
                        value={preset.hex}
                        label={preset.name}
                      />
                    ))}
                  </HctColorPickerPresets>

                  <HctColorPickerSliders className="mt-3 flex flex-col gap-2">
                    <HctColorPickerSlider channel="hue" label="Hue" />
                    <HctColorPickerSlider channel="chroma" label="Chroma" />
                    <HctColorPickerSlider channel="tone" label="Tone" />
                  </HctColorPickerSliders>

                  <div className="mt-3">
                    <HctColorPickerHexField label="Seed hex" />
                  </div>
                </HctColorPicker>

                <div className="flex flex-col gap-2">
                  <label className="text-on-surface-variant text-xs font-medium">
                    MCU Scheme Variant
                  </label>

                  <Select
                    variant="outlined"
                    value={variant}
                    onValueChange={(next) => setVariant(next as ThemeVariant)}
                  >
                    <SelectTrigger className="h-11">
                      <SelectValue placeholder="Choose variant" />
                    </SelectTrigger>
                    <SelectContent>
                      {THEME_VARIANT_SPECS.map((item) => (
                        <SelectItem key={item.id} value={item.id}>
                          {item.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>

                  {activeVariantMeta && (
                    <p className="text-on-surface-variant text-xs">
                      {activeVariantMeta.summary}
                    </p>
                  )}
                </div>

                <div className="flex flex-col gap-1.5">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-on-surface-variant font-medium">
                      Contrast Level
                    </span>
                    <span className="text-on-surface font-mono tabular-nums">
                      {contrastLevel.toFixed(1)}
                    </span>
                  </div>

                  <Slider
                    aria-label="Contrast level"
                    min={-1}
                    max={1}
                    step={0.5}
                    value={contrastLevel}
                    onValueChange={setContrastLevel}
                  />
                </div>

                <div className="flex items-center justify-between gap-3">
                  <span className="text-on-surface-variant text-xs font-medium">
                    Spec Version
                  </span>

                  <SegmentedButton
                    variant="tabs"
                    size="sm"
                    className="w-36"
                    value={specVersion}
                    onValueChange={(next) => {
                      if (next) setSpecVersion(next as ThemeSpecVersion)
                    }}
                  >
                    <SegmentedButtonItem value="2025">2025</SegmentedButtonItem>
                    <SegmentedButtonItem value="2021">2021</SegmentedButtonItem>
                  </SegmentedButton>
                </div>
              </div>
            </PopoverContent>
          </Popover>

          <SegmentedButton
            variant="glass"
            size="sm"
            value={mode}
            onValueChange={(next) => {
              if (next) setMode(next as GalleryViewMode)
            }}
          >
            <SegmentedButtonItem
              value="light"
              aria-label="Light mode"
              className="gap-1 px-2.5"
            >
              <LightModeRounded className="size-4" />
              <span className="hidden md:inline">Light</span>
            </SegmentedButtonItem>
            <SegmentedButtonItem
              value="dark"
              aria-label="Dark mode"
              className="gap-1 px-2.5"
            >
              <DarkModeRounded className="size-4" />
              <span className="hidden md:inline">Dark</span>
            </SegmentedButtonItem>
            <SegmentedButtonItem
              value="split"
              aria-label="Split Light and Dark mode"
              className="gap-1 px-2.5"
            >
              <SplitscreenLeftRounded className="size-4" />
              <span className="hidden md:inline">Split</span>
            </SegmentedButtonItem>
          </SegmentedButton>
        </GlassControlGroup>

        <GlassControlGroup className="hidden gap-1 px-1.5 md:flex">
          <SegmentedButton
            variant="glass"
            size="sm"
            value={viewport}
            onValueChange={(next) => {
              if (next) setViewport(next as GalleryViewportPreset)
            }}
          >
            <SegmentedButtonItem
              value="full"
              aria-label="Full width"
              title="Full width"
              className="px-2"
            >
              <WidthFullRounded className="size-4" />
            </SegmentedButtonItem>
            <SegmentedButtonItem
              value="expanded"
              aria-label="Expanded desktop (1280px)"
              title="Expanded (1280px)"
              className="px-2"
            >
              <DesktopWindowsRounded className="size-4" />
            </SegmentedButtonItem>
            <SegmentedButtonItem
              value="medium"
              aria-label="Medium tablet (840px)"
              title="Medium (840px)"
              className="px-2"
            >
              <TabletRounded className="size-4" />
            </SegmentedButtonItem>
            <SegmentedButtonItem
              value="compact"
              aria-label="Compact mobile (390px)"
              title="Compact (390px)"
              className="px-2"
            >
              <PhoneAndroidRounded className="size-4" />
            </SegmentedButtonItem>
          </SegmentedButton>
        </GlassControlGroup>

        <GlassControlGroup className="gap-1 px-1.5">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="glass"
                icon
                aria-pressed={xraySlots}
                aria-label="Toggle data-slot X-Ray"
                className={cn(
                  'size-9',
                  xraySlots &&
                    'bg-secondary-container text-on-secondary-container',
                )}
                onClick={() => setXraySlots(!xraySlots)}
              >
                <GridViewRounded className="size-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              X-Ray data-slot boundaries ({xraySlots ? 'On' : 'Off'})
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="glass"
                icon
                aria-pressed={reducedMotion}
                aria-label="Simulate prefers-reduced-motion"
                className={cn(
                  'size-9',
                  reducedMotion &&
                    'bg-secondary-container text-on-secondary-container',
                )}
                onClick={() => setReducedMotion(!reducedMotion)}
              >
                <AutoAwesomeMotionRounded className="size-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              Simulate Reduced Motion ({reducedMotion ? 'On' : 'Off'})
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="glass"
                icon
                aria-pressed={reducedTransparency}
                aria-label="Simulate prefers-reduced-transparency"
                className={cn(
                  'size-9',
                  reducedTransparency &&
                    'bg-secondary-container text-on-secondary-container',
                )}
                onClick={() => setReducedTransparency(!reducedTransparency)}
              >
                <BlurOffRounded className="size-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              Simulate Reduced Transparency (
              {reducedTransparency ? 'On' : 'Off'})
            </TooltipContent>
          </Tooltip>
        </GlassControlGroup>
      </div>
    </div>
  )
}
