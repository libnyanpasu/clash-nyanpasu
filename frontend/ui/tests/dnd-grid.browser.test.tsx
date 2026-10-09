import { act, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { DndGrid, DndGridItem, DndGridRoot } from '@nyanpasu/ui/dnd-grid'

test.for([
  { x: 330, y: 230, expected: { x: 3, y: 2 } },
  { x: 650, y: 230, expected: null },
])(
  'source unmount preserves the drop destination: %j',
  async ({ x, y, expected }, { onTestFinished }) => {
    const onDrop = vi.fn()
    const container = document.createElement('div')
    document.body.append(container)
    const root = createRoot(container)
    onTestFinished(() => {
      root.unmount()
      container.remove()
    })

    function Harness() {
      const [open, setOpen] = useState(true)
      return (
        <DndGridRoot>
          <div
            style={{
              position: 'fixed',
              left: 0,
              top: 0,
              width: 600,
              height: 400,
            }}
          >
            <DndGrid
              gridId="target"
              items={[]}
              disabled={false}
              minCellSize={100}
              gap={0}
              onExternalDrop={onDrop}
              className="target"
            >
              {() => null}
            </DndGrid>
          </div>
          {open && (
            <div
              style={{
                position: 'fixed',
                left: 0,
                top: 450,
                width: 200,
                height: 100,
              }}
            >
              <DndGrid
                gridId="source"
                items={[{ id: 'widget', x: 0, y: 0, w: 1, h: 1 }]}
                disabled={false}
                sourceOnly
                dragIdPrefix="library:"
                minCellSize={100}
                gap={0}
                onSourceDragStart={() => setOpen(false)}
                className="source"
              >
                {(item) => <DndGridItem id={item.id}>Widget</DndGridItem>}
              </DndGrid>
            </div>
          )}
        </DndGridRoot>
      )
    }

    const style = document.createElement('style')
    style.textContent = '.target, .source { width: 100%; height: 100%; }'
    document.head.append(style)
    onTestFinished(() => style.remove())
    await act(async () => root.render(<Harness />))
    await expect
      .poll(() => container.querySelector('[role="button"]'))
      .not.toBeNull()
    const source = container.querySelector('[role="button"]')!
    const pointer = (
      target: EventTarget,
      type: string,
      clientX: number,
      clientY: number,
    ) =>
      target.dispatchEvent(
        new PointerEvent(type, {
          bubbles: true,
          pointerId: 1,
          pointerType: 'mouse',
          isPrimary: true,
          button: 0,
          buttons: type === 'pointerup' ? 0 : 1,
          clientX,
          clientY,
        }),
      )

    await act(async () => {
      pointer(source, 'pointerdown', 30, 480)
    })
    await act(async () => {
      pointer(document, 'pointermove', 40, 480)
    })
    await expect.poll(() => container.querySelector('.source')).toBeNull()
    await act(async () => {
      pointer(document, 'pointermove', x, y)
    })
    await act(async () => {
      pointer(document, 'pointerup', x, y)
    })

    if (expected) {
      expect(onDrop).toHaveBeenCalledExactlyOnceWith('widget', expected)
    } else {
      expect(onDrop).not.toHaveBeenCalled()
    }
  },
)
