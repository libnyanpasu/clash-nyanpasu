# Proxies redesign: M3E group navigation, group folders, and drag-and-drop order

**Date:** 2026-10-24
**Baseline:** `main @ c34aae6fc`, which includes the Profiles redesign
(`docs/spec/2026-10-22-profiles-redesign/design.md`, PRs #5949–#5956).
**Status:** Draft for discussion. Nothing here is implemented yet.
**Mockup:** [Proxies Redesign Mockup](https://claude.ai/artifact/QoWRRuihgruA3kqKtE5U5t)
(a private Design canvas).
**Issue:** #5836, "Allow drag-and-drop reordering of proxy groups on the
Proxies page".

## 1. Background and problems

The Profiles redesign moved navigation to the shared M3E `NavigationList`
(`@nyanpasu/ui/navigation-list`) and explicitly left Proxies for later (Profiles
spec §2, non-goals). The Proxies sidebar still predates it.

- **Sidebar** (`proxies/_modules/proxies-navigate.tsx`).
  - Every group is a flat `fab` button with its own active styling and a 64px
    height. It shares nothing with the Settings and Profiles navigation.
  - The summary (type chip, pin, current node, delay) is a bespoke row inside the
    button, so a long group name or node name competes with the delay for width.
  - There is no heading, no grouping, and no way to reorder: groups appear in the
    order of the active configuration (Issue #5836).
- **Many groups.** Subscriptions routinely define 20 to 60 groups (regions,
  services, fallbacks). The user's main selector is one row in a long flat list.
- **Order is local to the page.** The Proxies page, the tray menu, and the
  navbar each read groups in the order the core reports them. A user's order
  would have to be applied in each place.

## 2. Goals and non-goals

### Goals

1. Use `NavigationList` for the group list, like Settings and Profiles.
2. Let the user **reorder** groups by drag and drop and from a menu.
3. Let the user put groups in **folders** in the sidebar.
   - A folder is a user-named, collapsible container of groups.
   - One level only. A group is in at most one folder.
   - Groups move between the list and folders, and between folders, by drag and
     drop or from a menu.
4. Store the arrangement **in the backend, per Config profile**, so that the
   Proxies page and the tray show the same order (§5).
5. Never change routing. The arrangement is presentation only: it does not write
   to the remote subscription, the profile's YAML, the runtime config, or any
   group's `proxies` list.
6. Offer **Reset order**, which returns to the order of the active configuration.

### Non-goals

- Changing the group page (`proxies/group/$name`): its header, node list,
  toolbar, search, sort, and delay test stay as they are.
- Nested folders, folder colors or icons, one group in several folders.
- Dashboard widgets. Their order is by traffic or by their own rules.
- Changing how the core reports groups or what `Proxies_Serialize` contains.
- Provider pages and the Global-mode single group.

### 2.1 Decisions

1. **Folders live in the sidebar only.** A folder has no page of its own. It
   expands and collapses in place, because its content is the group items the
   user already navigates between. (Profile folders have a page because their
   content is a card grid; here there is nothing more to show.)
2. **Backend, per profile.** The arrangement is stored in the profile document,
   keyed by the Config profile's uid. Switching profile switches arrangement,
   and deleting a profile deletes its arrangement. This follows Profile folders
   (Profiles spec §5), including "no schema revision bump".
3. **Order applies to the Proxies page and the tray.** The navbar has no group
   list today, so there is nothing to apply there.
4. **Groups are identified by name.** Clash identifies a group only by its name.
   The arrangement therefore stores names and tolerates names that appear and
   disappear (§5.2).
5. **Group icons stay images.** `group.icon` is rendered inside the
   `NavigationListItemIcon` shape (§3.2).

## 3. Sidebar

```text
Groups                              [⋮]   ← list actions: Reset order, expand/collapse all
  ├─ Proxy          Select · Auto Select          86 ms     /main/proxies/group/Proxy
  ├─ Auto Select    Url test · JP-02              54 ms
  └─ Fallback       Fallback · SG-01             112 ms
Folders                             [+]   ← new folder
  ├─ ▾ Streaming    3 groups
  │     ├─ Netflix  Select · US-03               180 ms
  │     ├─ Disney+  Select · HK-01                92 ms
  │     └─ YouTube  Select · Auto Select          74 ms
  └─ ▸ Regions      5 groups
```

- **Two groups, like Profiles.** "Groups" holds ungrouped groups, in the user's
  order. "Folders" holds the folders. The user's few key groups stay on top and
  the clutter goes in folders.
- **Group item.**
  - Icon: the group's image, or a tonal glyph by type when the group has none.
  - Headline: the group name, one line, ellipsis. The name is never squeezed by
    the delay.
  - Supporting: the type chip and, when the choice is pinned (`group.fixed`), the
    pin icon stay fixed in front; the current node follows in `TextMarquee`.
  - Trailing: the delay chip of the current node.
- **Folder item.**
  - A heading row of the folder's container: a chevron, the neutral folder icon,
    the name, and `N groups`. Clicking it expands or collapses the folder. It
    carries `aria-expanded`.
  - Its groups are items of the same rounded container, with the same left edge
    as the heading and no extra indent, so the folder reads as one block. Only
    the ends of the block get the large radius.
  - A collapsed folder still takes a dropped group.
  - Expanded state is a per-device preference (`useFolderCollapsed` pattern from
    Profiles), not part of the backend arrangement.
- **Selected item.** A selected group morphs its glyph to a cookie, as everywhere
  else. If it sits in a collapsed folder, the folder expands so the selection is
  visible.
- **Mobile.** `/main/proxies` shows this list full width. There is no sidebar
  drop target on mobile, so mobile reorders through the item menu (§4.2).
- **Empty folder.** A folder with no groups shows one muted line, "Drag groups
  here".

### 3.1 Row layout under width pressure

The 340px sidebar has 16px side padding, a 32px icon, 12px gaps and a delay chip
of about 60px, leaving about 150px for text. Group names and node names are
long (`🇭🇰 HK IEPL 03 x0.5`). The name truncates with an ellipsis and the
supporting line scrolls (`TextMarquee`); the chip never shrinks.

### 3.2 Shared component changes

`NavigationListItemIcon` takes an image child (the Profiles spec §3 deferred it
to this work): an `<img>`/`CacheImage` is shown whole and as it is, with no
background color, no clipping and no cropping (`object-contain`). It sits
outside the tonal shape and does not morph when its item is selected; only
its loading placeholder is a circle. The tonal
color classes and the cookie morph apply only to glyph children. `NavigationListItemText` gains `supportingStart`, fixed content in
front of the scrolling supporting line (the type chip and the pin), and a
headline that truncates with a `title`. A new `NavigationListItemTrailing` slot
sits after `NavigationListItemText` and holds the delay chip. No other change to
the components' API.

## 4. Interactions

### 4.1 Drag and drop

Uses `@dnd-kit/react` like the Profiles page: `useSortable` for groups and
folders, a `DragDropProvider` owned by the Proxies route, and the shared
sensors (`@/utils/dnd-sensors`).

| Drag     | Drop on                  | Result                                        |
| -------- | ------------------------ | --------------------------------------------- |
| a group  | a position in "Groups"   | reorder; leaves its folder if it was in one   |
| a group  | a position in a folder   | moves into the folder at that position        |
| a group  | a folder heading         | appends to the folder (expanded or collapsed) |
| a folder | a position among folders | reorders folders                              |

- **Drag sources.** A group item is dragged by the whole item, and a folder by
  its heading. A click still opens the group or toggles the folder, so a mouse
  drag starts only after the pointer moved 8px (`FOLDER_ITEM_DRAG_SENSORS`).
- **Drop targets.** The "Groups" list and each open folder's list are sortable
  sections, so a group takes the place under the pointer. A folder heading is a
  drop target of its own, and an empty folder's list takes the group.
- **Feedback.** The lists render the drag preview themselves: the other items
  make room, so the slot of the dragged item shows where it will land. The
  dragged item is raised (`data-dragging`), and the folder heading under the
  pointer gets the primary ring (`data-drop-target`). Both states exist in
  `NavigationListItem`. There is no separate insertion bar.
- **One commit per drop.** The drag previews the move locally and commits the
  whole arrangement once on drop. A drop that changes nothing commits nothing.
  Escape cancels without a commit.
- **Pointer and keyboard.** Keyboard dragging uses dnd-kit's keyboard sensor, with
  announcements in the Paraglide messages the Profiles page uses.
- **Right-to-left and touch.** Not specially handled; the Profiles page does not
  either. Touch users use the menu.

### 4.2 Menus

- **Group menu** (context menu, and the item's action on touch): _Move to
  folder_ (submenu: each folder, _No folder_, _New folder…_), _Move up_, _Move
  down_. Move up and down are bounded by the group's list or folder.
- **Folder menu**: _Rename_, _Move up_, _Move down_, _Delete folder_. Deleting a
  folder returns its groups to "Groups", after the existing ones, in their
  folder order.
- **List actions** (the ⋮ in the "Groups" heading): _Reset order_, _Expand all
  folders_, _Collapse all folders_. _Reset order_ is disabled when no
  arrangement is stored.
- **Reset order** removes the stored arrangement for the active profile: groups
  return to the core's order and the folders are deleted. It asks for
  confirmation, because it deletes folders.
- **New folder** opens the name dialog used by the Profiles folders
  (`FolderNameDialog` is reused, moved to a shared location if the import
  boundary requires it).

Failures (a rejected commit) show the error dialog the rest of the app uses.
There are no toasts.

## 5. Data model and backend

### 5.1 Types (`nyanpasu-config`)

```rust
pub struct ProxyGroupFolder {
    pub id: FolderId,            // reuses the profile folder id type
    pub name: String,
    pub members: Vec<String>,    // group names, display order in the folder
}

pub struct ProxyGroupLayout {
    pub folders: Vec<ProxyGroupFolder>,
    pub ungrouped: Vec<String>,  // group names, display order
}

// Profiles
#[serde(default, skip_serializing_if = "IndexMap::is_empty")]
pub proxy_group_layouts: IndexMap<ProfileId, ProxyGroupLayout>,
```

- **Keyed by the Config profile uid.** The runtime's groups come from the active
  Config profile, its transforms, and the global transforms. The arrangement
  belongs to the profile the user selected, so it is keyed by `current` when it
  is read and written.
- **Additive field, no schema bump.** A build that predates it reads the file
  and drops the field the next time it saves. Same reasoning as Profile folders
  (Profiles spec §5.1).
- **Specta.** The hand-maintained `ProfileDocument` shadow type gains the field.
- **Strict validation, in `Profiles::validate`.**
  - the key is an existing Config profile
  - folder ids are unique
  - folder names are non-empty after trimming
  - a name appears at most once in the whole layout (this also covers a
    duplicate inside one folder)
  - group names are non-empty

  Group names are _not_ validated against any runtime, because groups are not
  part of the profile document.

- **Load-time repair.** Like `repair_folders`: drop entries for missing
  profiles, keep the first folder of a duplicated id, drop repeated names, name
  empty folders "Folder". A bad value never stops the app from starting.
- **Deleting a profile** removes its entry in the same candidate.
- **Copying a profile** does not copy the arrangement. Group names usually
  match, but the copy is a different profile, and Profiles folders do not copy
  membership either.

### 5.2 Resolving the arrangement against the live groups

A pure function in `nyanpasu-config`:

```rust
pub fn resolve_group_layout(
    layout: &ProxyGroupLayout,
    live: &[String],            // group names in the core's order
) -> ResolvedGroupLayout;       // folders with live members, ungrouped, in display order
```

- A stored name that is not live is **ignored**, not deleted, so a group that
  vanishes for one refresh keeps its place when it returns. The stored list
  drops it at the next commit (the frontend commits the resolved arrangement).
- A live name the layout does not mention is **appended** to "Groups", in the
  core's order, so a new group shows up where the user will see it.
- A name in two places resolves to its first place.
- The flat order used by the tray is the sidebar from top to bottom: the
  ungrouped names, then each folder's members in folder order.

The same rules are implemented once more in TypeScript for the page
(`resolveGroupLayout`), because the page renders sections and the tray only the
flat order. Both implementations share one table of cases in their tests, which
is the contract.

### 5.3 Actor, client, RPC

| Operation               | Message / RPC                                                                          | Notes                                                                                                             |
| ----------------------- | -------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Replace the arrangement | `SetProxyGroupLayout { profile, layout }` → `set_proxy_group_layout(profile, layout)`  | One commit. A folder missing from `layout` is deleted. An empty layout removes the entry (that is Reset order).   |
| Create a folder         | `CreateProxyGroupFolder { profile, name, members }` → `create_proxy_group_folder(...)` | The actor mints the id like `create_profile_folder`. Listed members leave their current place in the same commit. |

- **RPC capability.** Both are `rpc(http)` mutations. They edit the profile
  document only: no OS effects, no outbound requests, no `AppHandle`. Registered
  as mutations in `specta_export.rs`; bindings regenerated.
- **Runtime impact.** `classify_profiles` never reads `proxy_group_layouts`, so a
  commit is `RuntimeImpact::None`: no regeneration, no core reload. A test pins
  this for both operations.
- **Reading.** The layout reaches the frontend through the existing profile
  document query. No new query.
- **Concurrency.** Last writer wins on the whole layout, as with
  `set_profile_layout`.

### 5.4 Tray

The tray menu lists groups from the proxies snapshot in the core's order. It
learns the active profile's arrangement and orders the groups with
`resolve_group_layout`.

- The tray reads the arrangement the same way it reads other profile-derived
  inputs, through the application effect plan, not through a global.
- A layout commit must rebuild the tray menu, so the arrangement becomes an input
  of the tray's desired state (`TrayMenuDesired`), and the plan diffing already
  refreshes the tray when an input changes.
- Folders do not appear in the tray menu: it is a flat list in the sidebar's
  top-to-bottom order.

### 5.5 Tests

- **Pure (`nyanpasu-config`):** round trip with and without the field; the wire
  shape; one case per validation error; each repair rule; the resolve table
  (known, unknown, new, duplicate, empty, folder with only unknown members).
- **Actor and client:** a valid layout commits once; an invalid one fails with
  `ValidationFailed`; an empty layout removes the entry; deleting a profile
  removes its entry; `Replace` keeps the field; startup with a malformed value
  repairs it.
- **Impact:** rows in the `actor.rs` case table for both operations expecting
  `None`.
- **Tray:** the menu order follows the arrangement; a vanished group is skipped;
  a new group is last.

## 6. Frontend

- `useProfile()` gains `setProxyGroupLayout` and `createProxyGroupFolder`, with
  an optimistic cache update and rollback on error, like the Profiles layout.
- `useProxyGroupSections()` returns `{ ungrouped, folders }` from the live groups
  (`useClashProxies`) and the active profile's stored layout, through the
  TypeScript `resolveGroupLayout`.
- Module layout (`proxies/_modules/`): `proxies-navigate.tsx` (the list),
  `group-nav-item.tsx`, `group-folder.tsx`, `proxies-dnd-provider.tsx`,
  `resolve-group-drop.ts` (pure drop rules, like `resolve-profile-drop.ts`),
  `use-group-sections.ts`. Names follow the Profiles modules.
- The route keeps its structure: `route.tsx` renders the sidebar and the outlet;
  `index.tsx` renders the list on mobile.
- i18n: new keys in all six locales, then a Paraglide recompile.
- Tests: pure tests for `resolveGroupLayout` and `resolveGroupDrop`; browser
  tests for the list (sections, expand and collapse, selection expands a
  collapsed folder, the menu moves a group, reset order).

## 7. Implementation phases and pull requests

Stacked, in the current checkout.

| PR  | Branch                    | Base                      | Content                                                                   |
| --- | ------------------------- | ------------------------- | ------------------------------------------------------------------------- |
| 1   | `feat/proxy-group-layout` | `main`                    | §5: model, validation, repair, resolve, actor, RPC, tray, tests, bindings |
| 2   | `feat/proxies-sidebar`    | `feat/proxy-group-layout` | §3, §4.2, §6: M3E list, folders, menus, reset, shared component changes   |
| 3   | `feat/proxies-dnd`        | `feat/proxies-sidebar`    | §4.1: drag and drop, announcements                                        |

The spec is the first commit of PR 1.

## 8. Open questions

1. Whether "Groups" should show the groups that are in no folder first, or the
   folders first. This spec puts the ungrouped groups first, so the main
   selector stays on top.
2. Whether _Reset order_ should keep the folders and only reset the order. This
   spec deletes both, because "return to the configuration" is the only state
   the user can predict.
