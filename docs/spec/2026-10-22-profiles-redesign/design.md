# Profiles redesign: M3E navigation, profile folders, detail, global transforms, and inspect

**Date:** 2026-10-22
**Baseline:** `main @ ca1814d0f`, which includes the settings redesign (#5917).
**Status:** Draft for discussion. Nothing here is implemented yet.
**Mockup:** [Profiles Redesign Mockup](https://claude.ai/artifact/GG3Z8pLtDtA2Y4jBKYDQ4J)
(a private Design canvas).

## 1. Background and problems

The settings redesign (`docs/spec/2026-10-22-settings-navigation/design.md`)
established an M3E navigation language: grouped containers with large radii at
the ends, colored icons, and a selected item that rounds every corner and morphs
its icon. The Profiles area predates it.

- **Sidebar** (`profiles/_modules/profiles-navigate.tsx`).
  - Items are flat `fab` buttons with hand-drawn composite icons.
  - A separator divides the list, and Global transforms and Inspect have no
    group.
  - `profile_profile_label_count` says "N profiles" for scripts and for global
    transforms.
- **No organization.** Users with many subscriptions can only reorder one flat
  list.
- **Profile Detail** (`$type/detail/$uid.tsx`).
  - The header shows only the name.
  - Six equal tiles mix edit, primary, destructive, and file actions. Delete
    sits next to Activate in the same style, and Edit Name appears twice.
  - Remote controls are split between two cards. Auto-update state appears in
    three places.
  - Sync history grows without bound and nests a scroller in the page.
  - A missing profile renders `null`.
  - Feedback uses native dialogs, with some titles hard-coded in English.
  - Refresh and Update Options share one task key
    (`update-remote-profile-${uid}`), so a spinner can show on the wrong button.
- **Global Transform** (`global/route.tsx`).
  - The page is a chain membership list in a card. A transform's content is
    edited only in the separate Monaco window, which takes four navigations to
    reach.
  - Apply and Reset sit below the fold and appear only when the chain is dirty.
  - The page does not say which config the chain applies to, or what runs before
    and after it.
- **Inspect** (`inspect/route.tsx`).
  - The page never says what it is for.
  - The default filter hides both ends of the pipeline: the source and the final
    output.
  - There is no final-config view and no field search.
  - Failed steps have no marker.
  - "Last applied snapshot" is a bare switch, far from what it changes.
  - The whole page scrolls while its viewers use `55vh` and `65vh` boxes.
- **Layout.** Every profiles page sits in one `AppContentScrollArea` with a
  `min-h-full` chain, and `PageTitle` takes about 160px. No page can fill the
  height and scroll its own regions.
- **Defects found during the audit** (fixed by the phase that touches the code):
  - `dxl:grid-cols-3` refers to an undefined breakpoint and has never applied.
  - `{...props}` is spread twice in `profiles-list.tsx`.
  - `data-slot="profiles-navigate"` sits on the card grid.

## 2. Goals and non-goals

### Goals

1. Apply the Settings M3E navigation to Profiles through a shared component (§3).
2. Add user-defined **profile folders**.
   - Each Config profile can be in at most one folder.
   - Folders can be created, renamed, deleted, and reordered.
   - Profiles can be reordered within a folder and moved between folders by
     drag and drop, or through a menu.
   - Folders persist in the backend profile document.
3. Redesign Profile Detail as an M3E page with a clear identity, primary actions,
   and grouped sections.
4. Turn Global Transform into a tool: the whole pipeline around the global
   chain, and a transform library. Code is edited only in the editor window.
5. Rebuild Inspect around the question "how was my runtime config built?":
   - the final output by default
   - stages
   - a status for each step
   - field search
6. Global Transform and Inspect fill the height. They have no `PageTitle`, and
   each region scrolls on its own.

### Non-goals

- Proxies (Issue #5836) gets its own design after Profiles.
- These folder features are out of scope:
  - colors or icons
  - tags
  - more than one folder per profile
  - folders for transforms
- Changing profile, transform, or runtime semantics. Backend changes are limited
  to folders (§5) and two Inspect fields (§8.5).
- Changing the profile card or the import FAB. Both keep their current design
  and behavior:
  - **The card.** This covers `GridViewProfile` and its parts: `ProfileCardFeatures`,
    `ProfileCardSubscription`, `ProfileCardTraffic`, and the footer actions.
    Its sizes and the active card's featured layout stay as they are. The
    featured layout is full width, `h-72`, `rounded-4xl`, with the
    `ActiveGradient` mesh. The active card also stays non-draggable.
  - **The FAB.** `ImportButton`, with its `FabMenu` of remote, local, and
    external sources, at the bottom right of the content.

  The only additions are the folder entries in the card's context menu (§6.3).
  The FAB also passes the current folder, so an import lands in it (§6.2).

- The editor window (`(editor)/editor/profile`) stays as it is.

### 2.1 Decisions already agreed

1. The navigation component is extracted in its own PR. #5917 has since merged,
   so the work branches from `main`.
2. Folders: one folder per profile, no color, Config profiles only.
3. Folders are stored in the backend `Profiles` document.
4. Proxy group order (a later piece of work) applies to the Proxies page, the
   tray, and the navbar, but not to dashboard widgets.
5. Profiles comes before Proxies.

## 3. Shared navigation component

`settings/_modules/settings-navigate.tsx` defines `SettingsNavigationGroup` and
`SettingsNavigationItem`. Profiles needs the same parts now, and Proxies will
need them next, so they move to `@nyanpasu/ui/navigation-list`. The same change
updates §5.5 of the Settings spec, which kept the parts local because they had a
single consumer.

```tsx
<NavigationList aria-label={…}>
  <NavigationListGroup label={…} action={<Button icon … />}>
    <NavigationListItem active={isActive} asChild>
      <Link to="…">
        <NavigationListItemIcon color="blue">{icon}</NavigationListItemIcon>
        <NavigationListItemText headline={…} supporting={…} />
      </Link>
    </NavigationListItem>
  </NavigationListGroup>
</NavigationList>
```

- `@nyanpasu/ui` has no router dependency, so `active` is a prop. Each app wrapper
  computes it with `useMatchRoute` and renders `Link`. The wrappers are
  `SettingsNavigationItem` in Settings and `ProfilesNavigationItem` in Profiles.
- Items forward `ref` and `data-*` props, so the app can make an item sortable
  (`useSortable`) or a drop target (`useDroppable`). The component styles two
  states:
  - `data-dragging`: raised, with a shadow.
  - `data-drop-target`: an inset primary ring over the state layer.

  The DnD logic itself stays in the app.

- `NavigationListGroup` takes an optional `action` in its heading row. Profiles
  uses it for the new-folder button.
- `NavigationListItemIcon` keeps the color variants and the circle-to-cookie
  morph. Image children, which Proxies will need for group icons, are added in
  the Proxies work, not here.
- **Slots.** The new names are:
  - `navigation-list`
  - `navigation-list-group`
  - `navigation-list-group-label`
  - `navigation-list-item`
  - `navigation-list-item-icon`

  The `settings-navigation-*` slots from #5917 are in no tagged release (only in
  nightlies), so the rename is acceptable. The same PR updates
  `tests/settings-navigate.browser.test.tsx` and the generated slot list.

- The sidebar shell matches Settings:
  - Background: `bg-surface-variant/50` in light, transparent in dark.
  - `min-w-72`.
  - A content ratio of 2.5 : 1. Folder names and counts need the extra width,
    and one ratio keeps the two areas consistent.

## 4. Information architecture

### 4.1 Sidebar

```text
Configs                                   [+] ← new folder
  ├─ All profiles          9 profiles           /main/profiles/profile
  ├─ ▸ Work                4 profiles           /main/profiles/folder/$folderId
  └─ ▸ Personal            2 profiles
Transforms
  ├─ JavaScript            3 scripts            /main/profiles/javascript
  ├─ Lua                   1 script             /main/profiles/lua
  └─ YAML merge            2 overlays           /main/profiles/merge
Pipeline
  ├─ Global transforms     2 enabled            /main/profiles/global
  └─ Inspect               Trace how the runtime config is built
```

| Item              | Icon                      | Color   |
| ----------------- | ------------------------- | ------- |
| All profiles      | `description-rounded`     | blue    |
| Folder            | `folder-rounded`          | neutral |
| JavaScript        | `javascript-rounded`      | yellow  |
| Lua               | `mdi:language-lua`        | cyan    |
| YAML merge        | `merge-rounded`           | orange  |
| Global transforms | `public`                  | purple  |
| Inspect           | `document-search-rounded` | green   |

- **Pipeline.** This new group replaces the separator. Its pages act on the
  active config's build, not on one profile.
- **Counts.** Each kind uses its own noun ("3 scripts", "2 overlays",
  "2 enabled"). Plurals go through Paraglide messages.
- **Folders.**
  - Folder items reorder by drag, and cards can be dropped onto them (§6.3).
  - Their icon is neutral, which keeps them under "All profiles" in visual
    weight.
  - A folder item has a context menu with Rename and Delete folder.
- **Mobile.** `/main/profiles` shows this navigation full width, as Settings
  does. The sidebar does not render on mobile, so mobile has no sidebar drop
  targets. The Move menu covers that case (§6.3).

### 4.2 Routes

| Path                               | Page                                             | Change                                |
| ---------------------------------- | ------------------------------------------------ | ------------------------------------- |
| `/main/profiles/$type`             | Config list (`profile`) or a transform kind list | the Config list gains folder sections |
| `/main/profiles/folder/$folderId`  | One folder                                       | **new**                               |
| `/main/profiles/$type/detail/$uid` | Profile Detail                                   | redesigned                            |
| `/main/profiles/global`            | Global Transform                                 | redesigned, fill layout               |
| `/main/profiles/inspect`           | Inspect, with `?step=<key>`                      | redesigned, fill layout               |

- **Why the folder view is a path, not `?folder=`.** `useMatchRoute`, which
  drives the navigation's active state, ignores search parameters. A path also
  gives each folder a stable deep link. The static `folder` segment takes
  precedence over the dynamic `$type`.
- **Shared list modules.** The list modules are used by both `$type` and
  `folder`. They move from `$type/_modules/` to `profiles/_modules/`, the
  nearest common ancestor.
- **Unknown folder id.** The page shows an `EmptyState` with a link to All
  profiles.
- **Back from Profile Detail.** The page goes back in router history, so it
  returns to the folder, All profiles, or the Global Transform pipeline it came
  from. With no history, it falls back to `/main/profiles/$type`.

## 5. Profile folders: data model and backend

### 5.1 Types (`nyanpasu-config`)

```rust
#[serde(transparent)] pub struct FolderId(pub String);

pub struct ProfileFolder {
    pub id: FolderId,
    pub name: String,
    pub members: Vec<ProfileId>,   // display order inside the folder
}

// Profiles
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub folders: Vec<ProfileFolder>,
```

- **One display order.** Every layout commit rewrites `items` to match the
  layout. That order is:
  1. each folder's members, in folder order
  2. ungrouped Config profiles
  3. transforms, in their existing relative order

  Consumers of the flat order, such as dashboard widgets, therefore list
  profiles in the order the user sees. `reorder_profiles_by_list` stays for the
  transform pages, which splice their own kind into the full order.

- **Validation** is strict, in `Profiles::validate`, and runs on every write in
  `prepare_candidate`. It adds these `ProfileValidationError` variants:
  - `FolderMemberNotFound`
  - `FolderMemberNotConfig`
  - `ProfileInMultipleFolders` (this also covers a duplicate in one folder)
  - `DuplicateFolderId`
  - `EmptyFolderName` (checked after trimming)

  Duplicate folder names are allowed.

- **Load-time repair.** `ProfilesClient` validates the loaded document and treats
  any failure as fatal (`client/profiles.rs:72-77`). A bad `folders` value would
  then stop the app from starting. That can happen after a hand edit, or after a
  downgrade that drops folders followed by an upgrade.

  So the loader first runs a pure `Profiles::repair_folders()`. It:
  - drops members that are dangling or are not Config profiles
  - keeps the first occurrence of a profile that appears twice
  - drops folders with a duplicate id
  - names empty folders "Folder"
  - logs what it changed

  The repaired document is written on the next commit. A repair never makes the
  rest of the document invalid.

- **Specta.** Add `folders` to the hand-maintained `ProfileDocument` shadow type,
  so the field reaches the bindings.
- **Schema revision.** No bump.
  - The field is additive. A build that predates folders reads the file and drops
    `folders` the next time it saves.
  - A bump would be worse: older builds refuse `profiles.yaml` above their head
    revision, so they could not start at all.

### 5.2 Actor and RPC

| Operation                                         | Message / RPC                                                                                          | Notes                                                                                                                                                                                                                                                                              |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Move, reorder, rename, delete, or reorder folders | `SetLayout { layout }` → `set_profile_layout(layout: ProfileLayout) -> MutationOutcome<()>`            | `ProfileLayout { folders, ungrouped }` must cover every Config profile exactly once. One commit replaces `folders` and rewrites the `items` order, so a cross-section drag is atomic. A folder left out of the layout is deleted, and its members must then appear in `ungrouped`. |
| Create a folder                                   | `CreateFolder { name, members }` → `create_profile_folder(name, members) -> MutationOutcome<FolderId>` | The actor mints `f` + `nanoid!(11)`, beside `generate_uid`. Listed members leave their current folders in the same commit, which supports "New folder…" from a card.                                                                                                               |
| Delete a profile                                  | existing `Delete`                                                                                      | Also removes the uid from `members` in the same candidate. Membership is not a referrer, so it never blocks a delete.                                                                                                                                                              |
| Replace a document                                | existing `Replace` (`actor.rs` ~1668)                                                                  | Copies `folders` along with the other document fields.                                                                                                                                                                                                                             |
| Create a profile in a folder                      | `NewProfileRequest.folder: Option<FolderId>`                                                           | An import from a folder view lands in that folder in the same commit.                                                                                                                                                                                                              |

- **RPC capability.** Both new commands are `rpc(http)` mutations. Like
  `reorder_profiles_by_list` and `set_global_transforms`, they edit the profile
  document only: no OS effects, no outbound requests, no `AppHandle`. Both are
  registered as mutations in `specta_export.rs`, and the bindings are
  regenerated.
- **Runtime impact.** `classify_profiles` (`impact.rs:398`) never reads `folders`
  or the order of `items`. A layout commit is therefore `RuntimeImpact::None`,
  with no regeneration and no core reload. A test pins this.
- **Concurrency.** The last writer wins on the whole layout, the same as
  `reorder_profiles_by_list`. `MutationProvider` refetches on the commit event,
  so other windows converge. An expected-revision guard can come later if it is
  ever needed.

### 5.3 Tests

- **Pure (`nyanpasu-config`):**
  - round trip with and without `folders`
  - the wire shape
  - one case per validation error
  - each `repair_folders` rule
  - the layout-to-order rewrite, which keeps the relative order of transforms
- **Actor and client:**
  - a valid layout commits once
  - an incomplete layout fails with `ValidationFailed`
  - a cross-folder move is one commit
  - `create_profile_folder` moves its listed members
  - deleting a profile removes its membership
  - `Replace` keeps `folders`
  - startup with a malformed `folders` repairs it instead of failing
- **Impact:** rows in the `actor.rs` case table for every layout operation, each
  expecting `None`.

### 5.4 Frontend data

`useProfile()` gains `setLayout` and `createFolder`.

A pure helper `profileSections(profiles)` returns:

```ts
{ folders: { folder, profiles }[], ungrouped: ConfigProfile[] }
```

It has unit tests.

On drop, the next layout comes from `move()` over a
`Record<FolderId | 'ungrouped', ProfileId[]>`, which dnd-kit 0.5.1 supports,
and goes to `setLayout` as a single commit.

The query cache is updated optimistically, so a card does not jump back while
the commit is in flight. On failure the cache rolls back and a snackbar shows
the error.

## 6. Profiles list with folders

### 6.1 All profiles

```text
┌ All profiles ─────────────────────────────────────────────────────────┐
│ ⌄ ▸ Work  4                                                      [⋯]  │
│ ┌ featured active card (existing style, full width) ─────────────────┐│
│ └────────────────────────────────────────────────────────────────────┘│
│ ┌card┐ ┌card┐ ┌card┐                                                  │
│ ⌄ ▸ Personal  2                                                  [⋯]  │
│ ┌card┐ ┌card┐                                                         │
│ ⌄ Not in a folder  3                                                  │
│ ┌card┐ ┌card┐ ┌card┐                                          (FAB) ⊕ │
└───────────────────────────────────────────────────────────────────────┘
```

**Cards and the active card**

- Cards are the existing `GridViewProfile`, unchanged.
- The active card keeps its featured layout. It sits first in its own
  section's grid and spans the full width, the same `order-first col-span-full`
  rule it uses today, now scoped to the section.
- The active card stays non-draggable. It moves between folders only through
  the Move menu (§6.3).
- If the active profile's folder is collapsed, its card is hidden with the
  rest of the folder.

**Sections**

- Folders come first, in their order, then "Not in a folder".
- A section heading has a collapse toggle, the name, a count, and an overflow
  menu with Rename and Delete folder.
- Collapse state is kept per folder under the KV key `profiles-folder-collapsed`.
- An empty folder shows a dashed drop zone.

**Grid**

- Two columns from `md`, three from `xl`. This replaces the inert `dxl`.

**Empty state**

- With no profiles at all, the existing `EmptyState` and FAB are shown, without
  sections.

### 6.2 Folder view

- The title is the folder name, with an overflow menu for Rename and Delete.
- Cards reorder within the folder.
- The existing import FAB is shown here as well. Its look and menu are
  unchanged. On this page it passes `folder` to `CreateProfileModal`, so a new
  profile lands in this folder.
- An empty folder shows an empty state with a hint to move profiles in from All
  profiles or with the Move menu.

### 6.3 Moving profiles

| Gesture                                                      | Result                                                                               |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| Drag a card within a section                                 | Reorders the section                                                                 |
| Drag a card into another section, or onto an empty drop zone | Moves it to that folder, at the drop position                                        |
| Drag a card onto a sidebar folder item (desktop)             | Appends it to that folder; the item shows `data-drop-target`                         |
| Drag a card onto "All profiles" in the sidebar (desktop)     | Removes it from its folder, appended to "Not in a folder"                            |
| Drag a sidebar folder item                                   | Reorders folders                                                                     |
| Card context menu → Move to folder ▸                         | Folders, "New folder…", and "Remove from folder"; uses the existing `ContextMenuSub` |
| Profile Detail overflow → Move to folder                     | The same choices, as a menu                                                          |

- **One provider.** A single `DragDropProvider` in `profiles/route.tsx` lets cards
  reach sidebar targets.
  - Draggables carry `type: 'profile' | 'folder'`.
  - Sidebar folders accept both types; sections accept `profile`.
  - `onDragEnd` checks `event.operation.target` before calling `move()`.
- **Activation.** Cards keep `WHOLE_ITEM_DRAG_SENSORS`, because their buttons are
  excluded and the card itself is not a link. The active card stays disabled
  for dragging, as it is today. Sidebar folder items are links, so
  their pointer sensor needs an 8px distance constraint: a click navigates, and
  a drag reorders.
- **Touch and keyboard.**
  - On touch, a long press opens the context menu, so the Move menu is the main
    path there.
  - The keyboard sensor sorts with the arrow keys.
  - Moves are announced through dnd-kit's accessibility announcements, with
    localized messages such as "Moved Office Backup to Personal, position 2
    of 3".
- **Creating a folder.** Two ways:
  - The heading `+` opens a `Modal` with a single name field.
  - "New folder…" in a card's Move menu creates the folder with that card in it.
- **Deleting a folder.** A confirmation says "N profiles will move to Not in a
  folder." Profiles are never deleted with a folder.
- **Feedback.** After a move or a folder delete, a snackbar offers Undo, which
  commits the previous layout again.

## 7. Profile Detail

### 7.1 Structure

The page is one centered column, `max-w-3xl`. It reads top-down: an overview
card in the profile card's style, then configuration, then maintenance. Below
the card, sections use the M3E grouped rows of the Settings pages
(`SettingsGroup`, `SettingsCard`).

```text
[←] My Subscription                     ✎   ← page header, outside the card
╭─ (ActiveGradient when active) ────────────────────────────╮
│ (Active) (Remote) (Auto update 2h) (▸ Work) (2 transforms)│
│ Remaining                                                 │
│ 76 GiB                                                    │
│ Total 200 GiB                                             │
│ ████████░░░░░░░░░░░░                                      │
│ Updated 2 hours ago   Expires 2026-11-14                  │
│ Nodes 120   Groups 14   Rules 3,412                       │
│ [⏻ Activate]                           (⟳) (✎) (⋯)        │
╰───────────────────────────────────────────────────────────╯
Subscription
  ╭ Subscription URL        https://sub.example…    ✎ ╮
  │ Auto update             Every 2 hours           › │
  │ User-Agent              clash-verge/v2.2.3      › │
  ╰ Download through        Direct                  › ╯
Transform chain                               2 enabled
  ╭ ⠿ 1  rename-nodes.js      JavaScript     ⋯ ╮
  │ ⠿ 2  cn-rules.yaml        YAML merge     ⋯ │
  ╰ ＋ Add transform                         ⌄ ╯
Sync history                         Next run in 1 h 12 min
  ╭ ✓ Today 14:02   Scheduled · 3.1 MB      Succeeded ╮
  │ ✕ Today 12:02   Scheduled · HTTP 403    Failed    │
  ╰ View full history                              ›  ╯
File
  ╭ Edit content                                   ›  ╮
  ╰ Show in file manager                           ↗  ╯
  ╭ Delete profile                                     ╮   ← error color
```

**Page header**

- It replaces `DetialHeader`.
- The back button and the title belong to the page, not to the profile card, so
  they sit in their own sticky header above the card.
- Left to right: a back icon button (its accessible name gives the destination,
  such as "Back to Work"), the profile name as a `title-large` heading, and a
  pencil that opens `ProfileNameEditor`. The duplicate Edit Name tile goes away.
- The header always shows, because Detail is a drill-down page. Top-level pages
  hide their back button on desktop.

**Overview card**

The card follows the profile card's style, so the list and the detail read as
the same object. It reuses the card's parts, not new look-alikes: the
`Card` shape, `ProfileCardChip`, the `ProfileCardSubscription` /
`ProfileCardTraffic` metric, `ProfileCardContentSummary`, the stroked Activate
button, and the round icon actions. When the profile is active it uses the
featured treatment: `rounded-4xl` and `ActiveGradient`. Its height follows its
content.

- **No name row.** The name lives in the page header, so the card starts with
  its chips.
- **Chips.** These are the same chips the card uses: Active (first, when the
  profile is active), source, auto update, and the transform count. A folder
  chip is added, linking to the folder page.
- **Metric.**
  - Remote profiles show Remaining, Total, the meter, Updated, and Expires.
    The accessible meter stays.
  - Local profiles and compositions show Recorded traffic.
- **Content counts.** Nodes, groups, and rules are always shown here.
- **Footer.**
  - Activate, as the stroked button, on the left. It is hidden while the
    profile is active, as on the card.
  - Round icon actions on the right: Update (remote only), View content, and
    More. More holds Move to folder and Show in file manager.
  - Delete is never in the card.
- **Quota.** `SubscriptionCard`'s quota logic and its test text
  (`Remaining quota 80.00%`, `80 B /100 B`) stay where its tests expect them.

**Subscription** (remote only)

- The URL row opens `SubscriptionUrlEditor`.
- Auto update, User-Agent, and Download through each open the existing
  `UpdateOptionEditor`, which keeps its tests. These rows are the only place that
  shows the auto-update state.
- Refresh and Update Options get separate task keys.

**Transform chain** (Configs only)

- The section uses `TransformChainList` (§7.2), with "N enabled" in the heading.
- While the chain is dirty, Apply and Reset show in a bar at the bottom of the
  section.
- A `LinearProgress` in the heading replaces the full-card blur mask.

**Sync history** (remote only)

- The section shows the three latest runs.
- "View full history" opens the existing `SyncHistoryCard` in a `Drawer`, as a
  side sheet on desktop and a bottom sheet on mobile. The component and its tests
  stay, and the page no longer grows without bound.

**File**

- The two rows are Edit content (`edit-document`) and Show in file manager
  (`folder-open`), with distinct icons.
- The section shows only when `hasMaterializedFile` is true.

**Delete**

- Delete is the last section, alone and in the error color.
- The confirmation names the profile and warns when it is active.
- After deleting, the page navigates back.

**States**

- Loading shows a skeleton of the header and two groups.
- An unknown uid shows an `EmptyState` with a link back.

**Feedback**

- Success and info messages use snackbars (`@nyanpasu/ui/snackbar`).
- Native dialogs are kept only for confirmations and blocking errors.
- The hard-coded "Update failed:", "Delete failed:", and "Error" titles in
  touched files become messages.

### 7.2 Shared `TransformChainList`

`TransformChainList` replaces the stacked two-column `TransformChainEditor`. It
shows the enabled chain as M3E rows. Each row has:

- a drag handle
- its order number
- the name
- the type
- an overflow menu: Open, Trace in Inspect, Move up, Move down, Remove

How transforms are added differs by page:

- Profile Detail uses an "Add transform" menu.
- Global Transform uses its library pane (§8.2).

The list lives in `profiles/_modules/`, the nearest common ancestor of its two
consumers. It keeps:

- the slots `profile-chain-editor-item` (with `data-uid`), `-order`, `-type`,
  `-toggle`, and `-empty`
- keyboard sorting
- the duplicate-chain `role="alert"`

`tests/transform-chain-editor.browser.test.tsx` changes from a cross-column drag
to adding from the menu and the library. The RPC bodies stay
`replace_profile_definition` and `set_global_transforms`.

### 7.3 Transform detail (JavaScript, Lua, YAML merge)

The page starts with the same page header (back, name, rename). Below it is an
overview card in the transform card's style:

- the source, type, and Global chips
- the View content and More actions

There is no Activate and no metric. The page then has these sections:

- **Used by.** The profiles whose chain contains this transform, and "Global
  transforms" when it is in the global chain. The list is computed from `items`
  and `global_transforms`, and each row is a link. The section heading warns
  that editing the content affects every row.
- **Subscription and Sync history**, when the transform is remote.
- **Delete.**

## 8. Global Transform and Inspect: full-height tools

### 8.1 Layout infrastructure

- **Opting in.** A tool page declares `staticData: { layout: 'fill' }`, which
  needs a one-time `StaticDataRouteOption` augmentation.
  `profiles/route.tsx` reads the leaf match. For a fill page it changes three
  things:
  - The content container gets `h-full` instead of `min-h-full`.
  - The outlet gets `min-h-0`.
  - The content viewport stops scrolling.

  Other profiles pages keep scrolling.

- **Page structure.** A fill page is a `flex min-h-0 flex-1 flex-col
overflow-hidden` column. A 56px `PageToolbar` replaces `PageTitle`, and every
  region below it is `min-h-0 flex-1` with its own `ScrollArea`, like
  `DataPage` (logs, rules, connections).
- **`PageToolbar`** lives in `profiles/_modules/page-toolbar.tsx`, because both
  of its consumers are under profiles. It has:
  - a back link on mobile only
  - a single-line `title-medium` title
  - a context slot
  - an actions slot
- **Documentation.** `docs/development/typescript.md` says long content scrolls
  rather than fixing pages to the viewport. It gains an exception: a tool page
  may opt in to the fill layout when every region scrolls internally. The
  `AnimatedOutletPreset` JSDoc gets the same note. That component changed on
  main (#5894, #5900: a leaving page stays on its own route), and the slide
  between fill and scrolling pages must be checked against the current version.
- **To verify while implementing:**
  - the slide between a fill page and a scrolling page
  - dnd-kit auto-scroll inside two internal scroll regions
  - the mobile back link

### 8.2 Global Transform

```text
┌ Global transforms │ Applies to: My Subscription │        ● Unapplied changes [Reset] [Apply] ┐
├ Pipeline                     Runs top to bottom ┬ Library                    [＋ New transform] ┤
│ 1 SOURCE      My Subscription              ›   │ [Search transforms…]                         │
│ 2 PROFILE     2 transforms                 ›   │ [All] [JavaScript] [Lua] [YAML]              │
│ 3 GLOBAL      (editable here)                  │   rename-nodes.js   JavaScript      ↗   ＋   │
│   ⠿ 1 ad-block.js      JavaScript   ↗  ⌕  −   │   dns-fix.js        JavaScript      ↗   ＋   │
│   ⠿ 2 region-sort.lua  Lua          ↗  ⌕  −   │   cn-rules.yaml     YAML merge      ↗   ＋   │
│ 4 NYANPASU    5 built-in steps             ⚙   │                                              │
└────────────────────────────────────────────────┴──────────────────────────────────────────────┘
  ↗ open in the editor window   ⌕ trace in Inspect   − remove   ＋ add
```

- **Pipeline.** The stages of a build, in order:
  1. **Source.** The active config, linking to its detail page.
  2. **Profile transforms.** That config's own chain, read only, linking to its
     detail page.
  3. **Global transforms.** Editable with `TransformChainList`, plus drag from
     the library.
  4. **Nyanpasu steps.** Read only, linking to Settings › Runtime Config.

  Their labels come from an exhaustive map typed
  `satisfies Record<BuiltinStepKind, string>`. A new step kind is then a compile
  error instead of a silent gap. The current order is: field filter, keep
  unmanaged, managed settings, defaults, then include-all expansion.

  Together, the stages show what the chain applies to and what runs around it,
  without calling the Inspect RPC.

- **Chain state.**
  - Apply and Reset sit in the toolbar. They are always visible and disabled
    while the chain is clean.
  - A tertiary chip marks unapplied changes.
  - A router blocker asks before leaving with unapplied changes.
- **Library.**
  - It lists the transforms that are not in the global chain, with search, a
    type filter, and an add button per row.
  - "New transform" opens the existing create modal with the transform kinds.
  - Two empty states: "no transforms exist" offers creation, and "all are
    enabled" says so.
- **Code editing.** This page has no preview and no inline editor. Every code
  edit, and every action on the code, goes through the existing editor window
  (`createEditorWindow`). Each chain and library row has an "Open in editor
  window" button. Chain rows also have "Trace in Inspect".
- **Narrow widths** (a container under 56rem, or mobile). The library stacks
  under the pipeline, and both scroll in one region.
- **Kept.**
  - `setGlobalTransforms` still replaces the whole list.
  - The `profile-global-transforms` slot.
  - The tested `set_global_transforms` body.

### 8.3 Inspect

```text
┌ Inspect │ [Latest build | Applied to core] ✓ Applied · mihomo · revision 42 │ [Find field…] [⟳] ┐
├ Build steps ───────────────────── 320px ┬ GLOBAL TRANSFORMS · STEP 4 OF 12 ────────────────────┤
│ Every step that built your runtime       │ ad-block.js            [Open profile] [Edit in Global]│
│ config, from the source to the core.     │ [Changes | Output | Logs 2]  Changed 1 field          │
│ SOURCE                                   │ rules  (chip → jumps in the diff)                     │
│   ⇥ My Subscription            Start     │ ┌──────────────────────────────────────────────────┐  │
│ PROFILE TRANSFORMS                       │ │ unified diff / YAML / logs, scrolls internally   │  │
│   ● rename-nodes.js         2 fields     │ │                                                  │  │
│ GLOBAL TRANSFORMS                        │ │                                                  │  │
│   ▲ ad-block.js              1 field     │ │                                                  │  │
│   ○ region-sort.lua        No change     │ │                                                  │  │
│ NYANPASU                                 │ │                                                  │  │
│   ● Field filter            3 fields     │ │                                                  │  │
│ RESULT                                   │ │                                                  │  │
│   ⚑ Final config                         │ │                                                  │  │
│   ⇤ Core effective config     Matches    │ └──────────────────────────────────────────────────┘  │
│ ☐ Only steps that changed something      │                                                        │
└──────────────────────────────────────────┴────────────────────────────────────────────────────────┘
```

**Purpose line.** The top of the step rail always says what the page shows and
what selecting a step does.

**Stages.** Nodes are grouped by `OperatorTag`, using the same M3E rows as the
sidebar:

| Stage              | Tags                                                                                                                       |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------- |
| Source             | `bare_root`, `file_config_root`, `composition_root`, `extend_proxies_step` (contributors indented under their composition) |
| Profile transforms | `scoped_transform`                                                                                                         |
| Global transforms  | `global_transform`                                                                                                         |
| Nyanpasu           | `builtin_transform` and `builtin_step` (all except `core_controller`)                                                      |
| Result             | the final node (§8.5), and `core_controller` when the build was applied                                                    |

**Default selection.**

- **Final config** is selected by default, on the Output tab. It answers "what
  does the core run" immediately.
- Any other step opens on Changes.
- Selection survives rebuilds and snapshot switches through a stable step key.
  For example:

  ```text
  global:<transform_profile_id>:<step_index>
  scoped:<host_profile_id>:<transform_profile_id>:<step_index>
  builtin:<step>
  root:<profile_id>
  extend:<composition_id>:<contributor_index>
  final
  core
  ```

  Node indexes are not stable across rebuilds, so they are not used.

- `?step=<key>` deep-links to a step.

**Step status.**

- The leading icon shows changed, no change, warning, error, start, final, or
  core. Warning and error come from `max_log_level` (§8.5).
- The trailing text is the field count or "No change".
- The full chain shows by default. "Only steps that changed something" is an
  opt-in filter, and it never hides Source or Result.

**Find field.**

- A path search, such as `dns.nameserver`, highlights the steps whose
  `changed_fields` match. It runs on the client against the summary.
- The viewers get find-in-text with next and previous.
- Selecting a changed-field chip scrolls the diff to that field.

**Snapshot.**

- "Latest build" and "Applied to core" become a labeled segmented control.
- A status chip explains the state in words: applied, not applied yet, or the
  core's view pending.

**Lanes.** The build is a graph: a node's `next` edges can fork (a composition's
contributors) and merge. Both views read one lane layout, computed from the visible
steps in display order. The first successor of a step continues in its lane, each
further successor opens a lane, and a step that several lanes lead into closes them.
A step hidden by the "only changed" filter is bypassed, so its neighbours stay joined.
A back edge, which cannot be drawn downward, is left out.

**List (branch gutter).** The rail shows the lanes in a gutter to the left of the rows,
outside the cards, so a row keeps its M3E shape and carries no coloured border.
Lane 0 is an outline-coloured line; further lanes are primary. A start is a hollow
ring, a merge a diamond, a fork a rounded square, and every other step a dot; the
selected step has a halo. The lines run through the stage labels. The gutter is as
wide as the lanes the build needs, 14px each, so a plain chain costs 14px.

**Graph.** A "List | Graph" segment in the toolbar switches the left pane. The graph
draws each stage as a tonal 28px band with a kicker, each step as a 44px node with its
status icon (no border or stripe), and each edge as a line with rounded elbows.
Selected is a primary fill, the field trace is primary-container and dims the rest,
and edges into or out of the selection are primary. Zoom out, zoom in and Fit sit at
the corner, with a legend of the status icons and the "only changed" switch. Selecting
a node opens the same step detail as the list, without a card layer around it.

**Kept.**

- `DiffViewer`: paging, `data-change`, and its tests.
- `YamlViewer`: the 4000-line highlight cap.
- Redaction on the backend.
- Invalidation on `configurationStatusChanged`.
- The profile chips: `useProfileLookup` and `useOpenProfile`.

**Implementation addendum.**

- **Narrow widths.** In a container narrower than `3xl` the step rail moves
  into a drawer that a "Steps" button in the toolbar opens. The detail panel
  then takes the full width.
- **Step keys.** The keys above gain a few forms so every node has one:
  - `core_controller` is `core`; `builtin_transform` nodes are
    `builtin-transform:<step_index>`.
  - A scoped transform whose role is not the selected one appends
    `@<composition_id>`.
  - Any key still shared by several nodes gets `~2`, `~3`, ... in node order.
- **Only steps that changed something.** It keeps Source, Result, the selected
  step, steps with changed fields, and steps whose `max_log_level` is a warning
  or an error.
- **Selection.** The selected step lives in the `?step=` search param and
  changes use `replace`, so the back button does not walk through steps.

### 8.4 Links between the tools

- Global Transform's Trace, and Trace in a `TransformChainList` row, open
  `/main/profiles/inspect?step=<key>`. An unknown key falls back to Final config.
- Inspect's "Edit in Global transforms" opens Global Transform with that
  transform selected.

### 8.5 Backend additions for Inspect (required)

Both fields are additive on `RuntimeInspection`, on top of main's `final_node`.
The bindings are regenerated.

- **`final_id: number`.** It removes the guess about which node is the final
  config.
- **`max_log_level: Option<StepLogLevel>` per node.** Without it, a failure can only
  be found by fetching every node's content, and each fetch restores and
  serializes the YAML.

The Inspect redesign depends on both.

## 9. Messages

- The new keys follow the feature namespaces:
  - `profile_nav_*`: sidebar groups and counts
  - `profile_folder_*`: folders, the Move menu, dialogs, and announcements
  - `profile_detail_*`: section labels and rows
  - `profile_global_*`: pipeline, library, and editor
  - `inspect_*`: stages, statuses, find, and the purpose line
- Every key goes into all six locales, next to its related keys.
- The misused `profile_profile_label_count` is replaced by kind-specific count
  messages.
- Keys that become unused are removed.

## 10. Implementation phases and pull requests

Every PR builds and passes its tests on its own, and runs the checks in
`docs/development/testing.md`.

| PR  | Content                                                                                                                         | Depends on |
| --- | ------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| 1   | `@nyanpasu/ui/navigation-list`; Settings moved onto it; Settings spec §5.5 updated                                              | `main`     |
| 2   | Profiles M3E sidebar with the Pipeline group and kind counts; fix `dxl`, the duplicate props, and the slot name                 | 1          |
| 3   | Backend folders: types, validation, load-time repair, actor messages, RPC, bindings, tests                                      | `main`     |
| 4   | Folder UI: folder route, sidebar folders, sections, DnD, Move menu, dialogs, Undo, create in a folder (cards and FAB unchanged) | 2, 3       |
| 5   | Profile Detail, `TransformChainList`, transform detail                                                                          | 2          |
| 6   | Fill layout, `PageToolbar`, Global Transform tool (pipeline and library)                                                        | 5          |
| 7a  | Inspect `final_id` and `max_log_level`                                                                                          | `main`     |
| 7   | Inspect redesign                                                                                                                | 6, 7a      |

PRs 3 and 7a can start at once. PR 5 can run beside PR 4.

## 11. Open questions

1. **Naming.** "Folder" in the UI and `ProfileFolder` in code. This avoids
   clashing with Clash proxy groups and with the sidebar's navigation groups.
