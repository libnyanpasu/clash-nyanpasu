import CancelRounded from '~icons/material-symbols/cancel-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import CodeRounded from '~icons/material-symbols/code-rounded'
import ContentCopyRounded from '~icons/material-symbols/content-copy-rounded'
import TaskAltRounded from '~icons/material-symbols/task-alt-rounded'
import { cva } from 'class-variance-authority'
import { useState, type ReactNode } from 'react'
import {
  Button,
  Card,
  CardContent,
  CardHeader,
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui'
import { cn } from '@nyanpasu/utils'
import { useThemeStudio } from './theme-studio-context'

export type StageSurfaceContext =
  'nested-well' | 'canvas' | 'card' | 'elevated' | 'filled' | 'glass'

const STAGE_SURFACE_OPTIONS: readonly {
  id: StageSurfaceContext
  label: string
}[] = [
  { id: 'nested-well', label: 'Nested Well' },
  { id: 'canvas', label: 'Canvas' },
  { id: 'card', label: 'Card' },
  { id: 'elevated', label: 'Elevated' },
  { id: 'filled', label: 'Filled' },
  { id: 'glass', label: 'Glass' },
]

const stageSurfaceVariants = cva(
  [
    'relative min-w-0 rounded-large p-6',
    'text-on-surface transition-colors duration-200',
  ],
  {
    variants: {
      surface: {
        'nested-well': 'bg-surface-container dark:bg-surface-container-high',
        canvas: 'bg-surface-container-low dark:bg-surface',
        card: 'bg-surface-container-lowest dark:bg-surface-container',
        elevated: 'bg-surface-container-high',
        filled: 'bg-surface-container-highest',
        glass:
          'bg-gradient-to-br from-primary-container/60 via-surface-container-low to-tertiary-container/60',
      },
    },
    defaultVariants: {
      surface: 'nested-well',
    },
  },
)

export function PageHeader({
  eyebrow,
  title,
  description,
  badges,
}: {
  eyebrow: string
  title: string
  description: string
  badges?: readonly string[]
}) {
  return (
    <header className="flex flex-col gap-3" data-slot="gallery-page-header">
      <div className="flex flex-wrap items-center gap-2">
        <span className="bg-secondary-container text-on-secondary-container rounded-full px-3 py-1 text-xs font-semibold tracking-wide uppercase">
          {eyebrow}
        </span>

        {badges?.map((badge) => (
          <span
            key={badge}
            className="bg-surface-container-highest text-on-surface-variant rounded-full px-2.5 py-1 font-mono text-xs"
          >
            {badge}
          </span>
        ))}
      </div>

      <div className="flex flex-col gap-1.5">
        <h1 className="text-on-surface text-3xl leading-tight font-semibold tracking-tight">
          {title}
        </h1>
        <p className="text-on-surface-variant max-w-3xl text-sm leading-6">
          {description}
        </p>
      </div>
    </header>
  )
}

export function SpecimenCard({
  title,
  description,
  slots,
  defaultSurface = 'nested-well',
  allowSurfaceSwitch = true,
  controls,
  code,
  children,
  className,
}: {
  title: string
  description?: string
  slots?: readonly string[]
  defaultSurface?: StageSurfaceContext
  allowSurfaceSwitch?: boolean
  controls?: ReactNode
  code?: string
  children: ReactNode
  className?: string
}) {
  const { mode, xraySlots, reducedTransparency } = useThemeStudio()
  const [surface, setSurface] = useState<StageSurfaceContext>(defaultSurface)
  const [showCode, setShowCode] = useState(false)
  const [copied, setCopied] = useState(false)

  const handleCopy = async () => {
    if (!code) return
    try {
      await navigator.clipboard.writeText(code)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1600)
    } catch {
      // Clipboard may be unavailable in restricted contexts.
    }
  }

  const renderStageBody = (forcedMode?: 'light' | 'dark') => (
    <div
      data-slot="gallery-stage-surface"
      data-xray-slots={xraySlots ? 'true' : undefined}
      data-simulate-reduced-transparency={
        reducedTransparency ? 'true' : undefined
      }
      className={cn(
        stageSurfaceVariants({ surface }),
        forcedMode === 'light' && 'gallery-theme-light light',
        forcedMode === 'dark' && 'gallery-theme-dark dark',
      )}
    >
      {forcedMode && (
        <span className="bg-surface-container-highest/90 text-on-surface-variant mb-4 inline-block rounded-full px-2.5 py-0.5 font-mono text-[11px] font-medium uppercase">
          {forcedMode}
        </span>
      )}

      <div className="min-w-0">{children}</div>
    </div>
  )

  return (
    <Card
      className={cn('overflow-visible', className)}
      data-slot="gallery-specimen-card"
    >
      <CardHeader className="flex flex-col items-stretch gap-3 px-6 pt-6 pb-0">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <h2 className="text-on-surface text-lg leading-6 font-semibold">
              {title}
            </h2>
            {description && (
              <p className="text-on-surface-variant text-sm leading-5">
                {description}
              </p>
            )}
          </div>

          <div className="flex shrink-0 flex-wrap items-center gap-2">
            {code && (
              <Button
                variant="basic"
                className="text-on-surface-variant h-8 min-w-0 gap-1.5 px-3 text-xs"
                onClick={() => setShowCode((current) => !current)}
              >
                <CodeRounded className="size-4" />
                <span>{showCode ? 'Hide JSX' : 'JSX'}</span>
              </Button>
            )}
          </div>
        </div>

        {(slots?.length || allowSurfaceSwitch) && (
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex flex-wrap items-center gap-1.5">
              {slots?.map((slot) => (
                <code
                  key={slot}
                  className="bg-surface-container text-on-surface-variant dark:bg-surface-container-high rounded-small px-2 py-0.5 text-[11px]"
                >
                  data-slot=&quot;{slot}&quot;
                </code>
              ))}
            </div>

            {allowSurfaceSwitch && (
              <SegmentedButton
                variant="tabs"
                size="sm"
                className="w-auto max-w-full"
                value={surface}
                onValueChange={(next) => {
                  if (next) setSurface(next as StageSurfaceContext)
                }}
              >
                {STAGE_SURFACE_OPTIONS.map((option) => (
                  <SegmentedButtonItem
                    key={option.id}
                    value={option.id}
                    className="flex-none px-2.5 text-[11px]"
                  >
                    {option.label}
                  </SegmentedButtonItem>
                ))}
              </SegmentedButton>
            )}
          </div>
        )}
      </CardHeader>

      <CardContent className="gap-4 px-6 pt-4 pb-6">
        {controls && (
          <div
            data-slot="gallery-specimen-controls"
            className="bg-surface-container-low dark:bg-surface-container-low/80 rounded-large flex flex-wrap items-center gap-3 px-4 py-3"
          >
            {controls}
          </div>
        )}

        {mode === 'split' ? (
          <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
            {renderStageBody('light')}
            {renderStageBody('dark')}
          </div>
        ) : (
          renderStageBody()
        )}

        {showCode && code && (
          <div
            data-slot="gallery-code-block"
            className="bg-surface-container-lowest border-outline-variant/40 rounded-large relative overflow-hidden border"
          >
            <div className="flex items-center justify-between px-4 py-2">
              <span className="text-on-surface-variant font-mono text-xs">
                TSX usage
              </span>

              <Button
                variant="basic"
                className="h-7 min-w-0 gap-1.5 px-2.5 text-xs"
                onClick={handleCopy}
              >
                {copied ? (
                  <CheckRounded className="size-3.5" />
                ) : (
                  <ContentCopyRounded className="size-3.5" />
                )}
                <span>{copied ? 'Copied' : 'Copy'}</span>
              </Button>
            </div>

            <pre className="text-on-surface overflow-x-auto px-4 pb-4 font-mono text-xs leading-5 select-text">
              <code>{code.trim()}</code>
            </pre>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

export function GuidelineComparison({
  title,
  rule,
  doTitle,
  doNote,
  doDemo,
  dontTitle,
  dontNote,
  dontDemo,
}: {
  title: string
  rule: string
  doTitle: string
  doNote: string
  doDemo: ReactNode
  dontTitle: string
  dontNote: string
  dontDemo: ReactNode
}) {
  return (
    <Card data-slot="gallery-guideline-comparison">
      <CardHeader className="flex flex-col items-start gap-1 px-6 pt-6 pb-0">
        <h3 className="text-on-surface text-lg font-semibold">{title}</h3>
        <p className="text-on-surface-variant text-sm">{rule}</p>
      </CardHeader>

      <CardContent className="grid grid-cols-1 gap-4 px-6 pt-4 pb-6 md:grid-cols-2">
        <div className="bg-surface-container dark:bg-surface-container-high rounded-large flex flex-col gap-3 p-4">
          <div className="flex items-center gap-2">
            <span className="bg-ok/15 text-ok inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-semibold">
              <TaskAltRounded className="size-3.5" />
              Do
            </span>
            <span className="text-on-surface text-sm font-medium">
              {doTitle}
            </span>
          </div>

          <div className="bg-surface-container-low dark:bg-surface rounded-medium p-4">
            {doDemo}
          </div>

          <p className="text-on-surface-variant text-xs leading-5">{doNote}</p>
        </div>

        <div className="bg-surface-container dark:bg-surface-container-high rounded-large flex flex-col gap-3 p-4">
          <div className="flex items-center gap-2">
            <span className="bg-error-container text-on-error-container inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-semibold">
              <CancelRounded className="size-3.5" />
              Don&apos;t
            </span>
            <span className="text-on-surface text-sm font-medium">
              {dontTitle}
            </span>
          </div>

          <div className="bg-surface-container-low dark:bg-surface rounded-medium p-4">
            {dontDemo}
          </div>

          <p className="text-on-surface-variant text-xs leading-5">
            {dontNote}
          </p>
        </div>
      </CardContent>
    </Card>
  )
}
