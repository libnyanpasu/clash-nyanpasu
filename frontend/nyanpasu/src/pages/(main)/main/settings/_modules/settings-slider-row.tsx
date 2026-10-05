import { useEffect, useState } from 'react'
import { Slider } from '@nyanpasu/ui/slider'

export function SettingsSliderRow({
  label,
  committedValue,
  min = 1,
  max,
  step = 1,
  unit,
  onCommit,
}: {
  label: string
  committedValue: number
  min?: number
  max: number
  step?: number
  unit?: string
  onCommit: (value: number) => void
}) {
  const [cachedValue, setCachedValue] = useState(committedValue)

  // sync the cached value with the committed value
  useEffect(() => {
    setCachedValue(committedValue)
  }, [committedValue])

  return (
    <>
      <div className="flex items-center justify-between">
        <span>{label}</span>

        <span>
          {cachedValue}
          {unit}
        </span>
      </div>

      <Slider
        value={cachedValue}
        min={min}
        max={max}
        step={step}
        onValueChange={(value) => {
          setCachedValue(value)
        }}
        onValueCommit={(value) => {
          if (value !== committedValue) {
            onCommit(value)
          }
        }}
      />
    </>
  )
}
