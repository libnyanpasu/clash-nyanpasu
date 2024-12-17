export const cleanDeepClickEvent = (
  e: Pick<MouseEvent, 'preventDefault' | 'stopPropagation'>,
) => {
  e.preventDefault()
  e.stopPropagation()
}
