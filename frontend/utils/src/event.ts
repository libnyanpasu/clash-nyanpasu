export const cleanDeepClickEvent = (
  event: Pick<MouseEvent, 'preventDefault' | 'stopPropagation'>,
) => {
  event.preventDefault()
  event.stopPropagation()
}
