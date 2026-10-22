const nextFrame = () =>
  new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))

const centerOf = (element: Element) => {
  const rect = element.getBoundingClientRect()

  return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
}

/**
 * Drags `source` over `target` in small steps, letting dnd-kit settle between
 * them. A single jump can end before the collision under the pointer has been
 * recomputed, which a person's slower hand never does.
 */
export const drag = async (source: Element, target: Element) => {
  const send = (
    type: string,
    on: Element,
    { x, y }: { x: number; y: number },
  ) =>
    on.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        cancelable: true,
        composed: true,
        pointerId: 1,
        pointerType: 'mouse',
        isPrimary: true,
        button: 0,
        buttons: type === 'pointerup' ? 0 : 1,
        clientX: x,
        clientY: y,
      }),
    )

  const from = centerOf(source)
  const to = centerOf(target)

  send('pointerdown', source, from)

  const steps = 12

  for (let step = 1; step <= steps; step += 1) {
    send('pointermove', document.body, {
      x: from.x + ((to.x - from.x) * step) / steps,
      y: from.y + ((to.y - from.y) * step) / steps,
    })
    await nextFrame()
    await nextFrame()
  }

  send('pointerup', target, to)
}
