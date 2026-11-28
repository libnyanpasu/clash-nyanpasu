import { cva } from 'class-variance-authority'
import { useId, useMemo, type ComponentProps } from 'react'
import {
  generateScopedThemeCssVars,
  ThemeMode,
  type ResolvedThemeMode,
  type Theme,
} from '@nyanpasu/theme'
import { cn } from '@nyanpasu/utils'

const NAV_ITEM_COUNT = 4

// The mockup can show a light scheme inside a dark page and the other way
// round, so tiers follow `mode` rather than the host's `dark:` variant.
const canvasVariants = cva('flex flex-col', {
  variants: {
    mode: {
      light: 'bg-surface-container-low',
      dark: 'bg-surface',
    },
  },
})

// Dashboard widget cards follow the A17 card tier of the real dashboard.
const widgetVariants = cva(
  'rounded-medium flex min-w-0 flex-col justify-between gap-2 p-2',
  {
    variants: {
      mode: {
        light: 'bg-surface-container-lowest',
        dark: 'bg-surface-container',
      },
    },
  },
)

// Title, menu and window-control placeholders in the header.
const titlebarMarkVariants = cva(
  'bg-on-primary-container/24 h-1.5 shrink-0 rounded-full',
)

const textMarkVariants = cva('bg-on-surface/16 h-1.5 rounded-full')

const sanitizeId = (id: string) => id.replace(/[^a-zA-Z0-9_-]/g, '')

export type ThemeMockupProps = Omit<ComponentProps<'figure'>, 'children'> & {
  palette: Theme
  mode: ResolvedThemeMode
}

/**
 * A small skeleton of the Clash Nyanpasu window painted with `palette` in
 * `mode`, whatever theme the surrounding page uses. Pass an `aria-label`.
 */
export function ThemeMockup({
  palette,
  mode,
  className,
  style,
  ...props
}: ThemeMockupProps) {
  const id = sanitizeId(useId())
  const resolved = mode === ThemeMode.DARK ? 'dark' : 'light'

  const css = useMemo(
    () =>
      generateScopedThemeCssVars(palette, `[data-theme-mockup="${id}"]`, mode),
    [palette, mode, id],
  )

  return (
    <figure
      role="img"
      data-slot="theme-mockup"
      data-theme-mockup={id}
      className={cn(
        canvasVariants({ mode: resolved }),
        'm-0 aspect-[16/10] w-full overflow-hidden',
        'border-outline-variant/40 rounded-large border',
        resolved === 'dark' ? 'dark' : '',
        className,
      )}
      style={{ colorScheme: resolved, ...style }}
      {...props}
    >
      <style>{css}</style>

      <div
        data-slot="theme-mockup-titlebar"
        className="dark:bg-primary-container bg-primary-fixed-dim flex h-6 shrink-0 items-center gap-1.5 px-2.5"
      >
        <span className={cn(titlebarMarkVariants(), 'w-12')} />
        <span className={cn(titlebarMarkVariants(), 'w-5')} />
        <span className={cn(titlebarMarkVariants(), 'w-5')} />

        {/* Window controls. */}
        <span className={cn(titlebarMarkVariants(), 'ml-auto size-2')} />
        <span className={cn(titlebarMarkVariants(), 'size-2')} />
        <span className={cn(titlebarMarkVariants(), 'size-2')} />
      </div>

      <div
        data-slot="theme-mockup-navbar"
        className="dark:bg-on-primary bg-primary-container flex h-7 shrink-0 items-center gap-1.5 px-2"
      >
        {Array.from({ length: NAV_ITEM_COUNT }, (_, index) => (
          <span
            key={index}
            className={cn(
              'h-3.5 w-10 rounded-full',
              index === 0
                ? 'dark:bg-primary-container bg-primary-fixed-dim'
                : 'bg-on-secondary-container/16',
            )}
          />
        ))}
      </div>

      <div
        data-slot="theme-mockup-canvas"
        className="grid min-h-0 flex-1 grid-cols-3 gap-2 p-2"
      >
        <div className={widgetVariants({ mode: resolved })}>
          <span className={cn(textMarkVariants(), 'w-2/3')} />
          <span className="bg-primary h-4 w-7 rounded-full" />
        </div>

        {/* The chart runs to the card edges like the real sparkline widget. */}
        <div
          className={cn(
            widgetVariants({ mode: resolved }),
            'col-span-2 overflow-hidden p-0',
          )}
        >
          <span className={cn(textMarkVariants(), 'm-2 mb-0 w-1/3')} />

          <svg
            aria-hidden
            viewBox="0 0 100 24"
            preserveAspectRatio="none"
            className="fill-primary/12 stroke-primary h-1/2 w-full"
          >
            <path
              d="M0 18 C14 14 22 20 34 12 S56 4 68 10 S88 20 100 6 V24 H0 Z"
              stroke="none"
            />
            <path
              d="M0 18 C14 14 22 20 34 12 S56 4 68 10 S88 20 100 6"
              fill="none"
              strokeWidth="2"
              vectorEffect="non-scaling-stroke"
            />
          </svg>
        </div>

        <div className={cn(widgetVariants({ mode: resolved }), 'col-span-2')}>
          <span className={cn(textMarkVariants(), 'w-1/4')} />

          <div className="flex gap-1.5">
            <span className="bg-secondary-container h-4 flex-1 rounded-full" />
            <span className="bg-tertiary-container h-4 flex-1 rounded-full" />
          </div>
        </div>

        <div className={widgetVariants({ mode: resolved })}>
          <span className={cn(textMarkVariants(), 'w-1/2')} />
          <span className="bg-primary-container rounded-medium size-7 self-end" />
        </div>
      </div>
    </figure>
  )
}
