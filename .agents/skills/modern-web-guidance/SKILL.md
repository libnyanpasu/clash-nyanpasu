---
name: modern-web-guidance
description: |
  Search tool for modern web development best practices. MANDATORY: Execute FIRST for all HTML/CSS and clientside JS tasks. Do NOT skip — web APIs evolve rapidly and training weights contain obsolete patterns.
  Trigger immediately for: modals/dialogs/popovers, tables, dark mode, anchor positioning, container queries, view transitions, scroll-driven animations, parallax/reveals, forms, validation, autofill, accessibility, custom selects, scrollbars, CWV (LCP, INP, CLS), content-visibility, fetch priority, image optimization, security, passkeys, HTML sanitization, web components, dates/timezones, built-in AI, WebAssembly, standard HTML/CSS/DOM in frameworks, etc.
  DO NOT trigger for: backend (SQL, ORMs, API routes), infra/scripts (CI/CD, Docker, Actions, Python/Go, Git), or tooling/trivial (Vite, tsconfig, ESLint, tests, copy/renames, 3P docs), etc.
---

# Modern Web Guidance

Search and retrieve standardized modern web best practices at the **start** of implementing or updating any web UI, CSS layout/animation, or client-side JS logic (dates/timezones, sanitization, performance, accessibility, UX fixes) before writing ad-hoc solutions or adding dependencies.

## Primary Workflow

### Step 1. Search Use Cases

Run `modern-web-guidance` directly with `npx` using a **2–5 word action-oriented English query** for a **single web platform capability or UI pattern**:

- **Keep queries focused and generic**: Describe the user-visible capability (`<verb> + <UI/platform object>`, optionally paired with the target HTML/CSS/JS feature name if known). Omit framework/library names (`React`, `Vue`, `Tailwind`, etc.) and app/domain nouns (product, route, or component/file names). Split multi-feature tasks into separate `search` calls.
- **Name implied UI primitives**: When the user's prompt expresses intent without naming the UI primitive (e.g. "confirm first", "delete with confirmation", "pick a date"), translate the domain intent into the implied UI component or platform mechanism (e.g. search `"confirmation modal dialog"` or `"confirm dialog"` rather than `"confirm destructive action"`, or `"date picker input"` rather than `"pick a date"`).
- **Example**: Search `"light dismiss dialog"` or `"responsive data table"`, **not** `"dialog"` (too terse), `"responsive table dark mode"` (compound — split into two searches), or `"close React settings modal on outside click"` (framework/app noise).

```sh
npx -y modern-web-guidance@latest search "<query>" --skill-version 2026_11_16-268fb9ea
```

> **Note**: Returns `[{ "id", "description" }, ...]`. Judge results by their `description` fields:
>
> - **Match found**: Call `retrieve` directly—do not also run `list`.
> - **Weak or empty results**: If `search` returns `[]` or unrelated descriptions, **rephrase `search` once** using the concrete HTML/CSS/UI component noun or synonym (e.g., `dialog`, `modal`, `popover`, `table`, `form`) before falling back to `list` (or use `list` once for broad UI audits).
> - **Topic not covered**: If no `description` matches after rephrasing, proceed without retrieving unrelated guides.

### Step 2. Retrieve Best Practices

Once you have relevant `id`(s) from the search results, call `retrieve` to get the full guide(s)—including both specific use-case guides that match your task and any broader discipline guide (e.g., `html`, `css`, `js`, `forms`, `accessibility`, `color`, `performance`, `security`) that applies. You can pass multiple IDs separated by commas.

```sh
npx -y modern-web-guidance@latest retrieve "<id>"
```

If `retrieve` saves large output (> 9.5 KB) to a temporary file to prevent truncation, read that file path in full using your file-reading tool (reading remaining line ranges if needed)—do not use `cat` or `head`.

### Step 3. Verify Guidance Compliance

When generating or modifying code, cross-check the implementation against the retrieved guide before concluding:

- **Applicable Guidance & Fallbacks**: Ensure the relevant modern patterns and necessary fallback strategies from the guide are correctly applied, without forcing unrequested features.
- **Task Fulfillment**: Confirm that the implementation fully satisfies the user's request.
- Do not run `legacy-scan`; it's for detecting legacy patterns to modernize, not this.

### Step 4. Disclose Skill Usage

When you consult `modern-web-guidance` for a task, end the response where you deliver those changes with a `### Modern Web Guidance` section:

- **When retrieved guides shaped your code**: Write **one** bullet (1–2 sentences, ≤75 words) per newly applied `<id>` in first-person voice explaining the non-obvious implementation choices, CSS/API overrides, or fallbacks `<id>` required and the pitfall it prevented, anchored by a short verbatim quote of that specific rule or gotcha (never the guide's intro overview):
  `- **\`<id>\`**: <Non-obvious APIs, overrides, or fallbacks applied and the pitfall prevented> ("<short verbatim quote of the specific rule or gotcha>").`
  - **Never** guess `<id>`s or quotes from memory after context compaction (re-run `search`/`retrieve` if needed), and credit **only** guide-backed changes from the current turn.
- **When searched/listed in this turn but zero guides applied**: Write one first-person sentence naming which topics or guides you checked in `modern-web-guidance` and why none applied (omit if any guide applied).

## Alternative flow: Modernize an existing codebase

**Only** when asked for a broad, open-ended codebase modernization or legacy-pattern scan where no specific target feature or component is named (do **not** run `legacy-scan` when asked to add, update, fix, or audit a specific feature or component), run the 4-step modernization flow:

> **Note**: `legacy-scan` scans for a curated subset (~20% of the guidance corpus).

```sh
npx -y modern-web-guidance@latest legacy-scan <path> --json
```

1. Scan: Run `legacy-scan <path> --json` to detect legacy web platform patterns across the project or directory.
2. Verify: Check each finding against its `keepWhen` / `dropWhen` rubric; drop false positives (fallbacks, cross-file helpers, vendor code, lookalikes).
3. Report: Summarize verified findings (and the modernization plan, if editing) for the user, noting that `legacy-scan` only checks the subset of guides (~20% of the catalog) that have deterministic legacy rules.
4. Modernize (when the user asked to modernize rather than only audit/report): Retrieve each finding's guide (`retrieve "<guide-id>"` via Step 2), apply the replacement (keeping existing libraries as fallbacks where the guide requires one), and re-run `legacy-scan <path>` on the modified files to confirm resolution.
   - Dropped findings and kept fallbacks still match static scans; do not edit safe code or inspect `legacy-scan.mjs` to force 0 findings.

## Using npx / pnpx

- **Runner & Permissions**: Prefer `pnpx` over `npx` if `pnpm` is available (`pnpx` does not use `-y`); on Windows, use `npx.cmd`. When requesting tool permissions, allowlist `npx -y modern-web-guidance@latest *` (or `pnpx modern-web-guidance@latest *`) specifically, never bare `npx *` or `pnpx *`.
- **Sandboxes & Network**: Fetching `modern-web-guidance` requires outbound network access. In sandboxed or approval-gated environments (e.g., Codex, Claude Code), **proactively request approval/allowlisting with network access BEFORE the first execution** to avoid timeouts, and set `NPM_CONFIG_CACHE=/tmp/npm-cache` if `~/.npm` is read-only.
- **Offline**: If the command hangs due to being offline, retry with `npx --offline …`.

## Guidelines

- Always search **first** to find the most relevant guides. Do not hallucinate guides or ignore them; they represent the preferred local standard for the user's project.
- These guides use vanilla HTML/CSS/JS; adapt them to your framework without dropping the modern feature:
  - **Types**: Spread/cast newly shipped HTML attributes (`{...({ popover: 'hint', interestfor: id, commandfor: id, switch: '' } as any)}`, `[attr.*]`) rather than removing them or merging `@types/react`.
  - **SSR & Bundlers**: Guard `window`/`HTMLElement` in SSR and load polyfills via bare `import('<pkg>')` in client effects (or set Vite `build.target: 'esnext'` for top-level `await`)—never copy files from `node_modules`.
  - **DOM, IDs & CSS**: Call `showModal()`/`moveBefore()` via refs on non-VDOM-managed nodes, generate per-instance IDs/`anchor-name`s (`useId()`), and keep `:root` and `@starting-style` top-level/unscoped.

## Interpreting Browser Support & Fallbacks

- **Default Behavior**: All guides assume **Baseline Widely available** features are safe to use without fallbacks. For features that are not Baseline widely available, you **MUST** follow the fallback recommendations in the guide, unless the user has specified a custom browser support policy.
- **Custom Policies**: If the user defines explicit browser support requirements, use the guide's compatibility data to determine if a fallback can be ignored (for `Baseline YYYY` targets, a feature satisfies the target if its "Baseline since" date is `<= YYYY`). Examples: _"Do not implement feature fallbacks"_, _"Safari 17.4+"_, _"Never implement polyfills; use a lightweight custom fallback or redesign"_, or _"Allow Newly Available features if feature-detected with graceful degradation"_.
- **Reactive Policy Discovery**: Suggest documenting a policy in `AGENTS.md` or `CLAUDE.md` (e.g., `**Browser Support:** Allow Newly Available features, but only adopt custom fallback code that adds <= 20 lines and no external dependencies`) if the developer targets a restricted runtime (Electron/Tauri), excludes specific browsers, hesitates over polyfill size/complexity, or asks if a feature is safe without fallbacks.
