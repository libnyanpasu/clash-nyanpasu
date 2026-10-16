import { act, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import {
  DndGrid,
  DndGridItem,
  DndGridRoot,
  DndGridSource,
  useDndGridRoot,
  type DndGridItemType,
} from '@nyanpasu/ui/dnd-grid'

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

function mountContainer(onFinished: (callback: () => void) => void) {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  const style = document.createElement('style')
  style.textContent = '.target, .source { width: 100%; height: 100%; }'
  document.head.append(style)
  onFinished(() => {
    root.unmount()
    container.remove()
    style.remove()
  })
  return { container, root }
}

test.for([
  { x: 330, y: 230, expected: { x: 3, y: 2 } },
  { x: 650, y: 230, expected: null },
])(
  'source unmount preserves the drop destination: %j',
  async ({ x, y, expected }, { onTestFinished }) => {
    const onDrop = vi.fn()
    const { container, root } = mountContainer(onTestFinished)

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

    await act(async () => root.render(<Harness />))
    await expect
      .poll(() => container.querySelector('[role="button"]'))
      .not.toBeNull()
    const source = container.querySelector('[role="button"]')!

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

test('a list source sizes its preview by the destination grid and drops there', async ({
  onTestFinished,
}) => {
  const onDrop = vi.fn()
  const { container, root } = mountContainer(onTestFinished)

  function DragSize() {
    const dims = useDndGridRoot()?.activeDrag?.dims
    return <output>{dims ? `${dims.width}x${dims.height}` : 'idle'}</output>
  }

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
          <DndGridSource
            itemId="widget"
            dragIdPrefix="library:"
            w={2}
            h={1}
            onSourceDragStart={() => setOpen(false)}
            className="list-source"
            style={{ position: 'fixed', left: 0, top: 450, width: 200 }}
          >
            Widget
          </DndGridSource>
        )}
        <DragSize />
      </DndGridRoot>
    )
  }

  await act(async () => root.render(<Harness />))
  const source = container.querySelector('.list-source')!
  await act(async () => {
    pointer(source, 'pointerdown', 30, 460)
  })
  await act(async () => {
    pointer(document, 'pointermove', 40, 460)
  })
  await expect.poll(() => container.querySelector('.list-source')).toBeNull()
  expect(container.querySelector('output')?.textContent).toBe('200x100')
  await act(async () => {
    pointer(document, 'pointermove', 330, 230)
  })
  await act(async () => {
    pointer(document, 'pointerup', 330, 230)
  })

  expect(onDrop).toHaveBeenCalledExactlyOnceWith('widget', { x: 3, y: 2 })
})

test.for([
  { disabled: false, animates: true },
  { disabled: true, animates: false },
])(
  'removed items animate out only while the grid is editable: %j',
  async ({ disabled, animates }, { onTestFinished }) => {
    const { container, root } = mountContainer(onTestFinished)
    const items: DndGridItemType[] = [
      { id: 'kept', x: 0, y: 0, w: 1, h: 1 },
      { id: 'removed', x: 1, y: 0, w: 1, h: 1 },
    ]

    function Harness({ shown }: { shown: DndGridItemType[] }) {
      return (
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
            items={shown}
            disabled={disabled}
            minCellSize={100}
            gap={0}
            className="target"
          >
            {(item) => (
              <DndGridItem id={item.id}>
                <span data-item={item.id}>{item.id}</span>
              </DndGridItem>
            )}
          </DndGrid>
        </div>
      )
    }

    const removed = () => container.querySelector('[data-item="removed"]')
    await act(async () => root.render(<Harness shown={items} />))
    await expect.poll(removed).not.toBeNull()

    await act(async () => root.render(<Harness shown={items.slice(0, 1)} />))
    if (animates) {
      // The exiting item stays where it was while it fades out.
      const element = removed()!.parentElement!
      expect(element.style.left).toBe('100px')
      expect(element.style.pointerEvents).toBe('none')
    } else {
      expect(removed()).toBeNull()
    }
    await expect.poll(removed).toBeNull()
    expect(container.querySelector('[data-item="kept"]')).not.toBeNull()
  },
)

test.for([
  {
    name: 'the grid resizes during the exit',
    before: { disabled: false, width: 600 },
    after: { disabled: false, width: 350 },
  },
  {
    name: 'editing starts in the render that removes items',
    before: { disabled: true, width: 600 },
    after: { disabled: false, width: 600 },
  },
])('exits finish when $name', async ({ before, after }, { onTestFinished }) => {
  const { container, root } = mountContainer(onTestFinished)
  const items: DndGridItemType[] = [
    { id: 'kept', x: 0, y: 0, w: 1, h: 1 },
    { id: 'removed', x: 1, y: 0, w: 1, h: 1 },
  ]

  function Harness({
    shown,
    disabled,
    width,
  }: {
    shown: DndGridItemType[]
    disabled: boolean
    width: number
  }) {
    return (
      <div style={{ position: 'fixed', left: 0, top: 0, width, height: 400 }}>
        <DndGrid
          items={shown}
          disabled={disabled}
          minCellSize={100}
          gap={0}
          className="target"
        >
          {(item) => (
            <DndGridItem id={item.id}>
              <span data-item={item.id}>{item.id}</span>
            </DndGridItem>
          )}
        </DndGrid>
      </div>
    )
  }

  await act(async () => root.render(<Harness shown={items} {...before} />))
  await expect
    .poll(() => container.querySelector('[data-item="removed"]'))
    .not.toBeNull()
  await act(async () =>
    root.render(<Harness shown={items.slice(0, 1)} {...after} />),
  )

  await expect
    .poll(() => container.querySelector('[data-item="removed"]'))
    .toBeNull()
})
