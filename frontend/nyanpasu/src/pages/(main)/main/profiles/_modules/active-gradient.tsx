import { RefObject, useEffect, useMemo, useRef, useState } from 'react'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import {
  blueFromArgb,
  greenFromArgb,
  redFromArgb,
} from '@material/material-color-utilities'
import { drawMeshGradient, type MeshGradientColor } from './mesh-gradient'

// The colors and shape of the backdrop, as the WebGL mesh gradient it
// replaces drew them.
const MAX_COLORS = 10
const DISTORTION = 0.5
const SWIRL = 0.1
// Seconds of the gradient's clock per second while it animates.
const SPEED = 1 / 3
// Milliseconds between pictures while it animates: 30 a second.
const FRAME_INTERVAL = 1000 / 30
// The canvas's shorter side in pixels. The gradient has no fine detail, so a
// small canvas scaled up by the browser shows it as well as a full one.
const SHORT_SIDE = 32

/**
 * The active card's backdrop. It stays still and animates only while the card
 * is hovered or focused. It is drawn on the CPU into a small canvas: creating
 * a WebGL context and compiling a shader made opening the page stall.
 */
export const ActiveGradient = ({
  cardRef,
}: {
  cardRef: RefObject<HTMLDivElement | null>
}) => {
  const { themePalette } = useExperimentalThemeContext()

  const colors = useMemo(
    () =>
      Object.values(themePalette.schemes.light)
        .slice(0, MAX_COLORS)
        .map((color): MeshGradientColor => [
          redFromArgb(color) / 255,
          greenFromArgb(color) / 255,
          blueFromArgb(color) / 255,
        ]),
    [themePalette],
  )

  const [animated, setAnimated] = useState(false)

  useEffect(() => {
    const card = cardRef.current

    if (!card) {
      return
    }

    const update = () =>
      setAnimated(card.matches(':hover') || card.matches(':focus-within'))
    const events = ['pointerenter', 'pointerleave', 'focusin', 'focusout']

    for (const event of events) {
      card.addEventListener(event, update)
    }

    return () => {
      for (const event of events) {
        card.removeEventListener(event, update)
      }
    }
  }, [cardRef])

  const canvasRef = useRef<HTMLCanvasElement>(null)
  // The gradient's clock, kept across hovers so it resumes where it stopped.
  const time = useRef(0)

  useEffect(() => {
    const canvas = canvasRef.current
    const context = canvas?.getContext('2d')

    if (!canvas || !context) {
      return
    }

    const options = { colors, distortion: DISTORTION, swirl: SWIRL }
    let image: ImageData | undefined

    const draw = () => {
      if (image) {
        drawMeshGradient(
          image.data,
          image.width,
          image.height,
          time.current,
          options,
        )
        context.putImageData(image, 0, 0)
      }
    }

    // The canvas keeps the card's aspect ratio at a fixed small size; the
    // observer reports the size after layout and before the frame paints.
    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect
      if (width === 0 || height === 0) {
        return
      }

      const scale = SHORT_SIDE / Math.min(width, height)
      const columns = Math.max(1, Math.round(width * scale))
      const rows = Math.max(1, Math.round(height * scale))
      if (image?.width !== columns || image.height !== rows) {
        canvas.width = columns
        canvas.height = rows
        image = context.createImageData(columns, rows)
        draw()
      }
    })
    observer.observe(canvas)

    let request = 0
    if (animated) {
      let last = performance.now()
      const tick = (now: number) => {
        // The colors drift a sixth of a unit a second; a new picture every
        // other frame or so looks the same as one every frame at a fraction
        // of the cost.
        if (now - last >= FRAME_INTERVAL) {
          time.current += ((now - last) / 1000) * SPEED
          last = now
          draw()
        }
        request = requestAnimationFrame(tick)
      }
      request = requestAnimationFrame(tick)
    }

    return () => {
      observer.disconnect()
      cancelAnimationFrame(request)
    }
  }, [colors, animated])

  return (
    <canvas
      ref={canvasRef}
      className="pointer-events-none absolute inset-0 size-full opacity-30"
      data-slot="profile-card-gradient"
      aria-hidden
    />
  )
}
