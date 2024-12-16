import { useSessionStorageState } from 'ahooks'
import { useLayoutEffect } from 'react'

export interface MousePosition {
  x: number
  y: number
}

export const useClickPosition = () => {
  const [mousePosition, setMousePosition] = useSessionStorageState<
    MousePosition | undefined
  >('use-click-position', {
    defaultValue: {
      x: 0,
      y: 0,
    },
  })

  useLayoutEffect(() => {
    const updateMousePosition = (ev: MouseEvent) => {
      setMousePosition({
        x: ev.clientX,
        y: ev.clientY,
      })
    }

    document.addEventListener('click', updateMousePosition, true)

    return () => {
      document.removeEventListener('click', updateMousePosition, true)
    }
  }, [setMousePosition])

  return mousePosition
}
