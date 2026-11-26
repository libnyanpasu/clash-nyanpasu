import PlayArrowRounded from '~icons/material-symbols/play-arrow-rounded'
import { motion } from 'motion/react'
import { useState } from 'react'
import { M3E_MOTION_SPEC } from '@nyanpasu/theme/spec'
import { Button, Card, CardContent, CardHeader } from '@nyanpasu/ui'
import { cn } from '@nyanpasu/utils'
import { PageHeader } from '../../studio/stage'

export function MotionPage() {
  const [toggled, setToggled] = useState(false)

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-motion-page">
      <PageHeader
        eyebrow="Foundations · Motion"
        title="M3E Spring Physics System"
        description="Spatial springs (dampingRatio < 1) animate position, size, and radius with tactile overshoot. Effects springs (dampingRatio = 1.0) animate opacity and color with zero bounce. Toggle Simulate Reduced Motion in the bottom dock to verify instant fallback."
        badges={['M3E_MOTION_SPEC', 'motion/react']}
      />

      <Card>
        <CardHeader className="flex flex-wrap items-center justify-between gap-3 px-6 pt-6 pb-0">
          <div>
            <h2 className="text-on-surface text-base font-semibold">
              Interactive Spring Race
            </h2>
            <p className="text-on-surface-variant text-xs">
              Compare Expressive vs Standard spatial springs and
              critically-damped effects springs side by side.
            </p>
          </div>

          <Button
            variant="flat"
            className="gap-1.5"
            onClick={() => setToggled((value) => !value)}
          >
            <PlayArrowRounded className="size-4" />
            <span>Trigger All Springs</span>
          </Button>
        </CardHeader>

        <CardContent className="flex flex-col gap-3 px-6 pt-4 pb-6">
          {M3E_MOTION_SPEC.map((spec) => (
            <div
              key={spec.id}
              className="bg-surface-container dark:bg-surface-container-high grid grid-cols-1 items-center gap-3 rounded-2xl p-4 lg:grid-cols-12"
            >
              <div className="lg:col-span-4">
                <div className="flex flex-wrap items-center gap-1.5">
                  <span className="text-on-surface font-mono text-xs font-semibold">
                    {spec.scheme}.{spec.speed}.{spec.kind}
                  </span>
                  <span
                    className={cn(
                      'rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase',
                      spec.kind === 'spatial'
                        ? 'bg-primary-container text-on-primary-container'
                        : 'bg-secondary-container text-on-secondary-container',
                    )}
                  >
                    {spec.kind}
                  </span>
                </div>
                <p className="text-on-surface-variant mt-1 text-xs">
                  {spec.usage}
                </p>
              </div>

              <div className="lg:col-span-5">
                <div className="bg-surface-container-highest relative flex h-12 items-center overflow-hidden rounded-full px-2">
                  {spec.kind === 'spatial' ? (
                    <motion.div
                      className="bg-primary text-on-primary grid h-8 w-20 place-items-center rounded-full text-xs font-semibold"
                      animate={{ x: toggled ? 180 : 0 }}
                      transition={{
                        type: 'spring',
                        stiffness: spec.stiffness,
                        damping: spec.damping,
                        mass: spec.mass,
                      }}
                    >
                      Spatial
                    </motion.div>
                  ) : (
                    <motion.div
                      className="bg-tertiary text-on-tertiary flex h-8 w-full items-center justify-center rounded-full text-xs font-semibold"
                      animate={{ opacity: toggled ? 1 : 0.25 }}
                      transition={{
                        type: 'spring',
                        stiffness: spec.stiffness,
                        damping: spec.damping,
                        mass: spec.mass,
                      }}
                    >
                      Effects opacity (dampingRatio 1.0)
                    </motion.div>
                  )}
                </div>
              </div>

              <div className="text-on-surface-variant font-mono text-[11px] lg:col-span-3">
                <div>
                  stiffness: {spec.stiffness} · damping: {spec.damping}
                </div>
                <div className="truncate opacity-80">
                  {spec.cssFallback} ({spec.cssDurationMs}ms)
                </div>
              </div>
            </div>
          ))}
        </CardContent>
      </Card>
    </div>
  )
}
