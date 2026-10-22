import { RefObject, useEffect, useMemo, useState } from 'react'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { hexFromArgb } from '@material/material-color-utilities'
import { MeshGradient, meshGradientMeta } from '@paper-design/shaders-react'

/**
 * The active card's backdrop. It stays still and animates only while the card
 * is hovered or focused: an animated shader redraws the whole card every frame.
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
        .slice(0, meshGradientMeta.maxColorCount)
        .map((color) => hexFromArgb(color)),
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

  return (
    <MeshGradient
      className="pointer-events-none absolute inset-0 size-full opacity-30"
      colors={colors}
      distortion={0.5}
      swirl={0.1}
      grainMixer={0}
      grainOverlay={0}
      speed={animated ? 1 / 3 : 0}
    />
  )
}
