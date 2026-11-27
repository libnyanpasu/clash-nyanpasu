import { SnackbarProvider, TooltipProvider } from '@nyanpasu/ui'
import { GalleryShell } from './shell/gallery-shell'
import { ThemeStudioProvider } from './studio/theme-studio-context'

export function App() {
  return (
    <ThemeStudioProvider>
      <TooltipProvider>
        <SnackbarProvider>
          <GalleryShell />
        </SnackbarProvider>
      </TooltipProvider>
    </ThemeStudioProvider>
  )
}
