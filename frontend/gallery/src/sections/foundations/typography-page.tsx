import { useState } from 'react'
import { TYPOGRAPHY_SCALE } from '@nyanpasu/theme/spec'
import {
  Card,
  CardContent,
  CardHeader,
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui'
import { PageHeader } from '../../studio/stage'

export function TypographyPage() {
  const [styleMode, setStyleMode] = useState<
    'compare' | 'baseline' | 'emphasized'
  >('compare')

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-typography-page">
      <PageHeader
        eyebrow="Foundations · Typography"
        title="30-Style Baseline & Emphasized Scale"
        description="M3 Expressive pairs 15 baseline styles with 15 emphasized styles that keep the exact same size and line-height while stepping up weight and tightening tracking for hero headings, active pills, and KPIs."
        badges={['Inter Variable', 'tabular-nums', 'CJK fallback']}
      />

      <Card>
        <CardHeader className="flex flex-wrap items-center justify-between gap-3 px-6 pt-6 pb-0">
          <div>
            <h2 className="text-on-surface text-base font-semibold">
              Type Scale Matrix
            </h2>
            <p className="text-on-surface-variant text-xs">
              Rendered from TYPOGRAPHY_SCALE in @nyanpasu/theme/spec.
            </p>
          </div>

          <SegmentedButton
            variant="tabs"
            size="sm"
            className="w-72"
            value={styleMode}
            onValueChange={(next) => {
              if (next)
                setStyleMode(next as 'compare' | 'baseline' | 'emphasized')
            }}
          >
            <SegmentedButtonItem value="compare">Compare</SegmentedButtonItem>
            <SegmentedButtonItem value="baseline">Baseline</SegmentedButtonItem>
            <SegmentedButtonItem value="emphasized">
              Emphasized
            </SegmentedButtonItem>
          </SegmentedButton>
        </CardHeader>

        <CardContent className="flex flex-col gap-2 px-6 pt-4 pb-6">
          {TYPOGRAPHY_SCALE.map((row) => (
            <div
              key={row.role}
              className="bg-surface-container dark:bg-surface-container-high flex flex-col gap-3 rounded-2xl p-4"
            >
              <div className="flex flex-wrap items-center justify-between gap-2 font-mono text-[11px]">
                <span className="text-primary font-semibold">{row.role}</span>
                <span className="text-on-surface-variant">
                  {row.sizePx}px / {row.lineHeightPx}px · baseline{' '}
                  {row.baselineWeight} ({row.baselineTracking}) · emphasized{' '}
                  {row.emphasizedWeight} ({row.emphasizedTracking})
                </span>
              </div>

              <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                {styleMode !== 'emphasized' && (
                  <div className="min-w-0">
                    <div className="text-on-surface-variant mb-1 text-[10px] uppercase">
                      Baseline
                    </div>
                    <div
                      className="text-on-surface truncate"
                      style={{
                        fontSize: `${row.sizePx}px`,
                        lineHeight: `${row.lineHeightPx}px`,
                        fontWeight: row.baselineWeight,
                        letterSpacing: row.baselineTracking,
                      }}
                    >
                      Nyanpasu Proxy 12.4 MB/s
                    </div>
                  </div>
                )}

                {styleMode !== 'baseline' && (
                  <div className="min-w-0">
                    <div className="text-on-surface-variant mb-1 text-[10px] uppercase">
                      Emphasized
                    </div>
                    <div
                      className="text-on-surface truncate"
                      style={{
                        fontSize: `${row.sizePx}px`,
                        lineHeight: `${row.lineHeightPx}px`,
                        fontWeight: row.emphasizedWeight,
                        letterSpacing: row.emphasizedTracking,
                      }}
                    >
                      Nyanpasu Proxy 12.4 MB/s
                    </div>
                  </div>
                )}
              </div>

              <div className="text-on-surface-variant text-xs">{row.usage}</div>
            </div>
          ))}
        </CardContent>
      </Card>

      {/* Monospace & Tabular Numerals */}
      <Card>
        <CardHeader className="flex flex-col items-start gap-1 px-6 pt-6 pb-0">
          <h2 className="text-on-surface text-base font-semibold">
            Network Telemetry & Script Fallbacks
          </h2>
          <p className="text-on-surface-variant text-xs">
            Live speeds, latencies, and logs use tabular-nums and monospace with
            ligatures disabled.
          </p>
        </CardHeader>

        <CardContent className="grid grid-cols-1 gap-4 px-6 pt-4 pb-6 md:grid-cols-2">
          <div className="bg-surface-container dark:bg-surface-container-high rounded-2xl p-4">
            <div className="text-on-surface-variant mb-2 text-xs font-medium">
              Tabular Numerals (KPIs)
            </div>
            <div className="text-on-surface space-y-1 font-mono text-2xl font-semibold tabular-nums">
              <div>↑ 14.82 MB/s · 24 ms</div>
              <div>↓ 88.09 MB/s · 108 ms</div>
            </div>
          </div>

          <div className="bg-surface-container dark:bg-surface-container-high rounded-2xl p-4">
            <div className="text-on-surface-variant mb-2 text-xs font-medium">
              CJK + Latin Pairing
            </div>
            <div className="text-on-surface space-y-1 text-base leading-6">
              <div>Rule Mode · 规则模式 · ルールモード · 규칙 모드</div>
              <code className="text-on-surface-variant text-xs">
                127.0.0.1:7890 -&gt; DIRECT (liga=0, calt=0)
              </code>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
