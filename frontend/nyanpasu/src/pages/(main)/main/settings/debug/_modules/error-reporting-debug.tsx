import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsCardHeader,
} from '../../_modules/settings-card'

const TEST_PREFIX = 'error reporting test'

/** Throws while rendering, so the route's error boundary catches it. */
const RenderCrash = (): never => {
  throw new Error(`${TEST_PREFIX}: render error`)
}

export default function ErrorReportingDebug() {
  const [crash, setCrash] = useState(false)

  const handleConsoleWarn = () => {
    console.warn(`${TEST_PREFIX}: console warning`)
  }

  const handleConsoleError = () => {
    console.error(
      new Error(`${TEST_PREFIX}: console error`, {
        cause: new Error(`${TEST_PREFIX}: root cause`),
      }),
    )
  }

  const handleUncaughtError = () => {
    setTimeout(() => {
      throw new Error(`${TEST_PREFIX}: uncaught error`)
    })
  }

  const handleUnhandledRejection = () => {
    Promise.reject(new Error(`${TEST_PREFIX}: unhandled rejection`))
  }

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardHeader>Error Reporting Test</SettingsCardHeader>

        <SettingsCardContent>
          <p className="text-sm">
            Each button emits one event, written to the application log under
            the clash_nyanpasu::frontend target within about a second. Repeats
            within a minute are merged into one counted record. The render error
            replaces this page with the error screen.
          </p>

          <div className="flex flex-wrap items-center gap-2">
            <Button variant="flat" onClick={handleConsoleWarn}>
              console.warn
            </Button>

            <Button variant="flat" onClick={handleConsoleError}>
              console.error
            </Button>

            <Button variant="flat" onClick={handleUncaughtError}>
              Uncaught Error
            </Button>

            <Button variant="flat" onClick={handleUnhandledRejection}>
              Unhandled Rejection
            </Button>

            <Button variant="flat" onClick={() => setCrash(true)}>
              Render Error
            </Button>
          </div>

          {crash && <RenderCrash />}
        </SettingsCardContent>
      </SettingsCardAnimatedItem>
    </SettingsCard>
  )
}
