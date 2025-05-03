import { PropsWithChildren } from 'react'
import { MDProvider } from '@libnyanpasu/material-design-react'
import { useSetting } from '@nyanpasu/interface'

export const ThemeMDProvider = ({ children }: PropsWithChildren) => {
  const { value } = useSetting('theme_color')

  return <MDProvider color={value ?? undefined}>{children}</MDProvider>
}

export default ThemeMDProvider
