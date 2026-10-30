import { useSyncExternalStore } from 'react'

export type ColorScheme = 'light' | 'dark'

function subscribe(onChange: () => void) {
  const observer = new MutationObserver(onChange)
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['class'],
  })
  return () => observer.disconnect()
}

const currentScheme = (): ColorScheme =>
  document.documentElement.classList.contains('dark') ? 'dark' : 'light'

/**
 * The color scheme the page is drawn in, which the theme provider marks with
 * the root's `dark` class.
 */
export function useColorScheme() {
  return useSyncExternalStore(subscribe, currentScheme)
}
