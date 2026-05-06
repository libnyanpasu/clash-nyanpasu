import { useCallback } from 'react'
import { compileCustomCss } from '@/utils/custom-css-compiler'
import { insertStyle } from '@/utils/styled'
import { useKvStorage } from '@interface/hooks'

const CUSTOM_CSS_KV_KEY = 'custom-css'
const CUSTOM_CSS_COMPILED_KV_KEY = 'custom-css-compiled'

const STYLE_ID = 'nyanpasu-custom-css'

export default function useCustomCss() {
  const [customCss, setCustomCss, { isLoading: isCustomCssLoading }] =
    useKvStorage<string | null>(CUSTOM_CSS_KV_KEY, null)

  const [compiledCss, setCompiledCss, { isLoading: isCompiledCssLoading }] =
    useKvStorage<string | null>(CUSTOM_CSS_COMPILED_KV_KEY, null)

  const tryInjectCss = useCallback(() => {
    if (compiledCss !== null) {
      insertStyle(STYLE_ID, compiledCss)
    }
  }, [compiledCss])

  /** Compile raw CSS source, save the compiled result to KV, and inject it into the DOM. */
  const upsert = useCallback(
    async (rawCss: string) => {
      const compiled = compileCustomCss(rawCss)
      await setCustomCss(rawCss)
      await setCompiledCss(compiled)
    },
    [setCustomCss, setCompiledCss],
  )

  return {
    value: customCss,
    upsert,
    isLoading: isCustomCssLoading || isCompiledCssLoading,
    tryInjectCss,
  }
}
