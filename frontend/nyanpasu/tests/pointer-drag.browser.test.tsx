import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors'
import { DragDropProvider, useDraggable, useDroppable } from '@dnd-kit/react'
import { drag } from './pointer-drag'

function Source() {
  const { ref } = useDraggable({
    id: 'source',
    sensors: WHOLE_ITEM_DRAG_SENSORS,
  })

  return (
    <div ref={ref} style={{ width: 100, height: 100 }} data-testid="source">
      Source
    </div>
  )
}

function Target() {
  const { ref } = useDroppable({ id: 'target' })

  return (
    <div ref={ref} style={{ width: 100, height: 100 }} data-testid="target">
      Target
    </div>
  )
}

test('pointer release reaches the sensor after the drop target unmounts', async () => {
  const ended = vi.fn()

  function Fixture() {
    const [showTarget, setShowTarget] = useState(true)

    return (
      <DragDropProvider
        onDragOver={({ operation: { target } }) => {
          if (target?.id === 'target') setShowTarget(false)
        }}
        onDragEnd={ended}
      >
        <Source />
        {showTarget && <Target />}
      </DragDropProvider>
    )
  }

  const view = await render(<Fixture />)
  const target = view.getByTestId('target').element()

  await drag(view.getByTestId('source').element(), target)

  expect(target.isConnected).toBe(false)
  expect(ended).toHaveBeenCalledOnce()
})
