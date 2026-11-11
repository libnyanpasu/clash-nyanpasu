# Modal composition audit

This audit covers every production `@nyanpasu/ui/modal` consumer as of
2026-11-11. The active rules are in
[TypeScript and React code style](../development/typescript.md#compose-modals-and-preserve-their-lifecycle).

A controlled boolean does not inherently disable Motion. The repeated problems
were missing triggers/origins, conditionally mounting an already-open root,
removing content before exit, resetting visible drafts on close, and forwarding
open state through layers that did not own the interaction.

## Shared component

| Finding                                                                      | Resolution                                                                                                              |
| ---------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Switch discarded slotted children                                            | Preserve its children so a switch used as a modal trigger retains the animated placeholder.                             |
| No local API for asynchronous completion                                     | `useModal` exposes the nearest root's state and setter.                                                                 |
| One shared layout ID for all triggers                                        | Give each trigger a stable origin; select the actual trigger when opening.                                              |
| Menu/tray entry has no persistent trigger                                    | `ModalAnchor` supplies an origin without opening on click.                                                              |
| No exit completion hook                                                      | `ModalContent.onExitComplete` supports deferred cleanup.                                                                |
| Cancel/close button props were incompletely forwarded                        | `ModalClose` forwards button props and supports `asChild`.                                                              |
| Pending forms need dismissal control                                         | `ModalContent.dismissible` blocks Escape/outside dismissal.                                                             |
| Multiple triggers could return focus to the wrong button                     | Capture the opener before autofocus and restore it after exit; an opening that interrupts exit uses the latest trigger. |
| Injecting `relative` could override an absolutely positioned slotted trigger | Merge the child's positioning classes before applying caller overrides.                                                 |

## Application inventory

Paths below are relative to `frontend/nyanpasu/src/`. A shared content component
uses the root composed by its feature rather than receiving an open boolean.

| Consumer                                                                             | Findings and final ownership                                                                                                                                                                                                       |
| ------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pages/(main)/main/_modules/folder-name-dialog.tsx`                                  | Externally controlled shared form without an origin. Both folder providers now own an uncontrolled root; add buttons are triggers, rename headings are persistent anchors. Form close actions and submission dismissal stay local. |
| `pages/(main)/main/_modules/filter-chips.tsx`                                        | More button had no trigger. Compose the button and content under an internal root; clear-all closes locally.                                                                                                                       |
| `pages/(main)/main/connections/_modules/column-settings-modal.tsx`                   | Open state forwarded from page through viewers/table. A page root now joins the toolbar trigger and table's content.                                                                                                               |
| `pages/(main)/main/connections/_modules/table-row.tsx`                               | **Controlled exception:** one detail dialog serves virtualized/live rows and remains independent of row mounting. Keep its root/content mounted.                                                                                   |
| `pages/(main)/main/logs/_modules/file-logs.tsx`                                      | Local open boolean duplicated root state. Root owns opening; trigger copies filter draft and submit closes through context.                                                                                                        |
| `pages/(main)/main/profiles/_modules/create-profile-modal.tsx`                       | Open props forwarded from import and library entries. Content now reads its composed root; pending submission prevents dismissal.                                                                                                  |
| `pages/(main)/main/profiles/_modules/leave-dialog.tsx`                               | **Controlled exception:** router/editor blocker owns the pending navigation decision.                                                                                                                                              |
| `pages/(main)/main/profiles/$type/detail/_modules/profile-field-dialog.tsx`          | Shared field shell forwarded open/cancel state. Shell now composes trigger/content/close; editors own roots and close successful saves locally.                                                                                    |
| `pages/(main)/main/profiles/$type/detail/_modules/profile-name-editor.tsx`           | Unnecessary controlled root and reset at close. Root owns state; draft resets on opening.                                                                                                                                          |
| `components/profile-import/import-url-dialog.tsx`                                    | **Controlled exception:** incoming URL/deep-link requests have a promise lifecycle and can replace a request. Provider retains the last request during exit and blocks cancellation mid-import.                                    |
| `pages/(main)/main/settings/about/credits/_modules/credit-license-modal.tsx`         | Page boolean and conditional content. Shared root with each package row as a trigger; content remains mounted and selection is retained.                                                                                           |
| `pages/(main)/main/settings/clash/control/_modules/external-controller-config.tsx`   | Trigger already existed; root state was duplicated only for completion. Close through local context.                                                                                                                               |
| `pages/(main)/main/settings/clash/control/_modules/core-secret-config.tsx`           | Same unnecessary local open state. Close through local context.                                                                                                                                                                    |
| `pages/(main)/main/settings/clash/control/_modules/web-ui.tsx`                       | Duplicated open state and draft reset during exit. Root owns state; reset on opening.                                                                                                                                              |
| `pages/(main)/main/settings/clash/core/_modules/latency-test-config.tsx`             | Duplicated open state. Trigger resets draft; close through context after both saves succeed.                                                                                                                                       |
| `pages/(main)/main/settings/clash/network/_modules/proxy-port-config.tsx`            | Duplicated open state in strategy form. Root owns state; reset on opening and close on successful apply.                                                                                                                           |
| `pages/(main)/main/settings/clash/runtime/_modules/field-filter-card.tsx`            | Already uses uncontrolled root/trigger/content/close; retained.                                                                                                                                                                    |
| `pages/(main)/main/settings/nyanpasu/interface/_modules/custom-css-name-dialog.tsx`  | Mounted permanently open and removed on close, truncating exit. Persistent feature/per-preset roots now host content; create/duplicate buttons are triggers and rename menu buttons are anchors. Failed saves keep the form open.  |
| `pages/(main)/main/settings/nyanpasu/public-ip/_modules/add-stun-server-dialog.tsx`  | Duplicated open state; cleared fields/results during exit. Root owns state; close invalidates late replies, next opening resets fields/results.                                                                                    |
| `pages/(main)/main/settings/nyanpasu/public-ip/_modules/ip-lookup-provider-list.tsx` | Provider selection drove open state and cleared endpoint content. Each switch composes its own root/trigger and persistent notice content; disabling bypasses opening.                                                             |
| `pages/(main)/main/settings/nyanpasu/updates/_modules/update-controls.tsx`           | Already composes root/trigger/content/close; retained.                                                                                                                                                                             |
| `pages/(main)/main/settings/system/proxy/_modules/transparent-proxy-button.tsx`      | Explanatory dialog opened through an unrelated boolean. Proxy button is now the trigger when explanation is needed; executable actions prevent opening.                                                                            |
| `pages/(main)/main/settings/system/service/_modules/system-service-ctrl.tsx`         | Two dialogs already composed correctly. Reset raw JSON mode on opening to preserve closing content.                                                                                                                                |
| `pages/(main)/main/settings/debug/_modules/block-task-viewer.tsx`                    | Already composed correctly; retained.                                                                                                                                                                                              |
| `pages/(main)/main/settings/debug/_modules/kv-storage.tsx`                           | Already composed correctly; retained.                                                                                                                                                                                              |

Native OS dialogs (`ask`, file pickers, panic windows) and `Drawer`/sheet controls
are separate APIs and are outside this Modal migration.

## Follow-up lifecycle fixes

A delayed subscription save reproduced an unguarded close path: closing and
reopening while the old request was pending let its completion affect the newer
interaction. Editing dialogs now block Escape, outside dismissal and close
buttons during saving, disable repeat submit clicks, and lock form submissions
that also accept Enter. Native submit events are prevented before checking the
lock, so repeated Enter presses cannot navigate the page. Failures retain the
draft and restore cancellation.
Controller/secret/Web UI drafts reset on opening, so refreshed backend values do
not clear the pending state midway through saving.

A second browser regression reproduced focus returning to the previous trigger
when another opening interrupted exit. Focus restoration now prefers the latest
recorded origin. The animation continues through the existing presence owner.

## Verification

- All 201 frontend test files passed: 1,234 tests (`--maxWorkers=2`), including the proxy folder's
  actual trigger-to-dialog and dialog-to-trigger translation and focus restoration.
- Shared-modal browser coverage exercises controlled and uncontrolled roots,
  separate trigger origins, persistent menu anchors, dismissal during pending
  work, and exit completion with retained content.
- Workspace typechecking, Prettier, Oxlint, frontend package boundaries, and the
  production web build passed. Oxlint still reports existing warnings outside
  the changed behavior; no new warnings were added.
- An existing log-entry animation assertion failed once in the default-concurrency
  run. Its isolated rerun and the full two-worker run passed without changing
  that test.
- Native Tauri and live HTTP integration were not run. Browser-mode tests use
  the existing mocked RPC boundary.
