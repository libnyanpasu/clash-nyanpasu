# Custom CSS items and bundled presets

## 1. Background and goals

Custom CSS was a single LESS/CSS string (KV keys `custom-css` and
`custom-css-compiled`) edited in one singleton editor window. It now supports:

1. Several named custom CSS items.
2. Any number of items enabled at once, in a user-controlled order.
3. Stylesheets bundled with the application ("presets").

The feature is frontend-only. It adds no RPC command and no actor; the one Rust
change is the editor window identity (section 5).

## 2. Decisions

1. **One KV key.** The whole state lives under `custom-css-items`, so a toggle, a
   rename and a reorder are each one atomic write, and no per-item key needs a
   dynamic hook. Each item stores its `source` and its `compiled` CSS.
2. **Cascade order.** Enabled presets apply first, in catalog order, then enabled
   items in list order. A user's own CSS therefore wins over a preset, and a later
   item wins over an earlier one.
3. **Reordering is required.** Items reorder by dragging (dnd-kit, as in the STUN
   server list) or through the row menu (move up / move down), which is the
   keyboard-accessible path.
4. **One editor window per item.** `EditorWindowType::CssEditor` now requires a
   `uid` (the item id). The window label is `editor-css-<uid>` and stays a
   singleton, so opening an item twice focuses the same window.
5. **Presets are read-only.** Only the ids of enabled presets are stored. A preset
   is compiled when first needed and cached for the process. "Duplicate to edit"
   copies a preset into a new, disabled item.
6. **A failing item or preset does not block the others.** An item compiles when
   it is saved, so a compile error rejects that save and stores nothing. A preset
   that fails to compile is skipped and logged.

## 3. Data model

```ts
interface CustomCssItem {
  id: string
  name: string
  enabled: boolean
  source: string // LESS/CSS as written
  compiled: string // standard CSS, injected as-is
}

interface CustomCssState {
  items: CustomCssItem[]
  enabledPresets: string[] // preset ids
}
```

Pure transitions and the cascade (`resolveActiveStyles`) live in
`src/utils/custom-css-items.ts` and are tested with plain values.

## 4. Injection

`CustomCssProvider` resolves the active styles whenever the state changes and
replaces every `<style>` whose id starts with `nyanpasu-custom-css-`:
`nyanpasu-custom-css-preset-<id>` and `nyanpasu-custom-css-item-<id>`.
`insertStyle` appends to `<head>`, so replacing all of them is what keeps the
cascade order correct after a reorder. Every window mounts the provider; KV change
events keep them in step.

## 5. Editor window

`create_editor_window('css-editor', uid)` opens the editor for one item. The route
`/editor/css?uid=<id>` reads the item from the state and saves through
`saveSource`, which compiles first. The editor's header shows the item name.

## 6. Settings UI

Settings → User interface → Custom CSS shows, top to bottom: a one-line
explanation of the order, the bundled presets (switch plus "Duplicate to edit";
hidden when the app ships none), and the user's items (sortable list, edit,
enable switch, rename, move, delete, and "Add CSS").

## 7. Bundled presets

The catalog is `src/assets/custom-css-presets/index.ts`. A preset is a `.less`
file next to it, imported with `?raw`, plus a catalog entry with a stable `id`
and message-backed name and description. Presets use `data-slot` selectors, which
are the styling contract in the TypeScript guide. The catalog ships empty; the
presets themselves are authored separately.

## 8. Migration

If `custom-css-items` has never been written, the provider carries a non-empty
legacy `custom-css` string over as one enabled item (id `legacy`, named "Custom
CSS"). The old keys are left in place so a downgrade still finds its CSS. Every
window runs this, so the write keeps whatever another window saved first, and the
fixed id makes concurrent runs write the same item.

## 9. Verification

- Vitest: migration, edit transitions, reorder, cascade order, a failing preset.
- Rust: the CSS editor needs a uid and opens once per item.
