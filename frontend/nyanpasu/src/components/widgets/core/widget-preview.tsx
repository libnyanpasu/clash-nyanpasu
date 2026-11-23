import { useEffect, useRef, useState } from 'react'
import { cn } from '@nyanpasu/utils'
import type { WidgetId } from '../widget-config'
import type { WidgetDefinition } from './define-widget'
import { getWidgetDefinition } from './registry'
import { WidgetPreviewHost } from './widget-host'

const PREVIEW_CELL = 84
const PREVIEW_GAP = 12

export type WidgetPreviewProps = {
  type?: WidgetId
  definition?: WidgetDefinition
  className?: string
}

/**
 * A preview stage with a fixed logical canvas. The inner widget renders at its
 * real grid size, so container queries and list capacity behave exactly as
 * they do on the dashboard; `scale` fits that canvas into the stage without
 * changing the layout the widget measures.
 */
export function WidgetPreview({
  type,
  definition,
  className,
}: WidgetPreviewProps) {
  const resolved =
    definition ?? (type !== undefined ? getWidgetDefinition(type) : null)

  if (!resolved) {
    throw new Error('WidgetPreview requires a widget definition or type')
  }

  const stageRef = useRef<HTMLDivElement>(null)
  const [scale, setScale] = useState(0.55)

  const logicalWidth =
    resolved.size.defaultW * PREVIEW_CELL +
    (resolved.size.defaultW - 1) * PREVIEW_GAP
  const logicalHeight =
    resolved.size.defaultH * PREVIEW_CELL +
    (resolved.size.defaultH - 1) * PREVIEW_GAP

  useEffect(() => {
    const stage = stageRef.current

    if (!stage) {
      return
    }

    const update = () => {
      const width = stage.clientWidth
      const height = stage.clientHeight

      if (width > 0 && height > 0) {
        setScale(Math.min(width / logicalWidth, height / logicalHeight, 1))
      }
    }

    update()

    const observer = new ResizeObserver(update)
    observer.observe(stage)

    return () => observer.disconnect()
  }, [logicalWidth, logicalHeight])

  return (
    <div
      ref={stageRef}
      data-slot="widget-preview-stage"
      data-widget-type={resolved.id}
      aria-hidden="true"
      inert
      className={cn(
        'pointer-events-none relative flex size-full items-center justify-center overflow-hidden select-none',
        className,
      )}
    >
      <div
        data-slot="widget-preview-canvas"
        className="shrink-0 origin-center"
        style={{
          width: logicalWidth,
          height: logicalHeight,
          transform: `scale(${scale})`,
        }}
      >
        <WidgetPreviewHost definition={resolved} />
      </div>
    </div>
  )
}
