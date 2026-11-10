import { PropsWithChildren, useEffect } from 'react'
import { CUSTOM_CSS_PRESETS } from '@/assets/custom-css-presets'
import useCustomCss, {
  LEGACY_CUSTOM_CSS_COMPILED_KV_KEY,
  LEGACY_CUSTOM_CSS_KV_KEY,
} from '@/hooks/use-custom-css'
import { m } from '@/paraglide/messages'
import { compileCustomCss } from '@/utils/custom-css-compiler'
import {
  isCustomCssStyleId,
  migrateLegacyCustomCss,
  resolveActiveStyles,
  type CustomCssPreset,
} from '@/utils/custom-css-items'
import { insertStyle, removeStyle } from '@/utils/styled'
import { useKvStorage } from '@nyanpasu/query'

// A preset's source never changes while the app runs, so compile it once.
const compiledPresets = new Map<string, Promise<string>>()

function compilePreset(preset: CustomCssPreset) {
  let compiled = compiledPresets.get(preset.id)

  if (!compiled) {
    compiled = compileCustomCss(preset.source)
    // Let a failed compile be retried instead of cached.
    compiled.catch(() => compiledPresets.delete(preset.id))
    compiledPresets.set(preset.id, compiled)
  }

  return compiled
}

/**
 * Keeps the injected `<style>` elements in step with the saved state: one per
 * enabled preset and item, in cascade order.
 */
export default function CustomCssProvider({ children }: PropsWithChildren) {
  const { state, isMigrated, isLoading, setState } = useCustomCss()

  const [legacySource, , { isLoading: isLegacySourceLoading }] = useKvStorage<
    string | null
  >(LEGACY_CUSTOM_CSS_KV_KEY, null)
  const [legacyCompiled, , { isLoading: isLegacyCompiledLoading }] =
    useKvStorage<string | null>(LEGACY_CUSTOM_CSS_COMPILED_KV_KEY, null)

  const isLegacyLoading = isLegacySourceLoading || isLegacyCompiledLoading

  // Carry the single CSS string of earlier versions over once. The old keys
  // stay, so a downgrade still finds its CSS. Every window runs this, so the
  // write keeps whatever another window saved first.
  useEffect(() => {
    if (isLoading || isLegacyLoading || isMigrated) {
      return
    }

    setState(
      (prev) =>
        prev ??
        migrateLegacyCustomCss(
          legacySource,
          legacyCompiled,
          m.custom_css_legacy_item_name(),
        ),
    )
  }, [
    isLoading,
    isLegacyLoading,
    isMigrated,
    legacySource,
    legacyCompiled,
    setState,
  ])

  useEffect(() => {
    if (isLoading) {
      return
    }

    let cancelled = false

    resolveActiveStyles(
      state,
      CUSTOM_CSS_PRESETS,
      compilePreset,
      (preset, error) => console.error(`Custom CSS preset ${preset.id}`, error),
    ).then((styles) => {
      if (cancelled) {
        return
      }

      // `insertStyle` appends, so replace every style to keep the cascade order.
      document.head
        .querySelectorAll<HTMLStyleElement>('style[id]')
        .forEach((element) => {
          if (isCustomCssStyleId(element.id)) {
            removeStyle(element.id)
          }
        })

      styles.forEach((style) => insertStyle(style.id, style.css))
    })

    return () => {
      cancelled = true
    }
  }, [isLoading, state])

  return <>{children}</>
}
