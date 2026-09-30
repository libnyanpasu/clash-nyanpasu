# WebKit 下位置伪类选择器导致的样式失效开销评审

> 背景：#5445 优化 proxies 页面切换分组的掉帧后，WebKit（macOS / Linux 的 webview 引擎）仍有一帧约 65 ms。用 `frontend/nyanpasu/perf/` 基准定位到，剩余开销的主要来源是 Tailwind 的 `space-*` / `divide-*` 工具类在 WebKit 中引发的大范围样式重算。本文记录原因、实测数据、方案评估与工具调研。
>
> 状态：采用方案 C（构建时拆分 `:where()`），实施于 #5448。

## 1. 结论

- WebKit 匹配 `:last-child` 时会给父元素打上 `ChildrenAffectedByLastChildRules` 标记；之后向该父元素追加子元素，原最后一个子元素的 last-child 状态改变，WebKit 对它调用 `invalidateStyleForSubtree()`，**整棵子树重算样式**。
- Tailwind v4 生成的是 `:where(.space-y-2 > :not(:last-child))`：组合器被包在 `:where()` 里。这种写法下 WebKit 无法凭祖先的类名提前排除规则，每个元素都会判断一次 `:not(:last-child)`，因此几乎所有父元素都带上了标记。结果是：应用内任何"追加子元素"的 DOM 操作，都会让前一个兄弟节点的整棵子树重算样式。
- 把同一规则写成 `:where(.space-y-2) > :where(:not(:last-child))`，特异度同为 0、匹配的元素完全相同，WebKit 下的开销即降为 0。问题的根源是**组合器被包在 `:where()` 里**，而不是位置伪类本身。
- 这是 WebKit 特有的开销。Chromium 下同样操作耗时不到 0.1 ms。
- 在 proxies 页面，切换分组时新页面被追加到旧页面之后，旧页面（约 500 个元素）整体重算样式，WebKit 下约 8 ms。
- Tailwind 无法替换这些工具类的选择器（见 §4.1）。采用方案 C：在 Tailwind 之后用 PostCSS 把 `:where(A > B)` 改写为 `:where(A) > :where(B)`，不改标记，样式不变。WebKit 下切换分组的同步耗时由 32–35 ms 降到 18–20 ms，最长一帧由 64–73 ms 降到 51–53 ms。

## 2. 机制（WebKit 源码）

以下均出自 WebKit `main` 分支（2026-09-30 读取）。

`Source/WebCore/css/SelectorChecker.cpp`：匹配 `:last-child` 时，无论是否命中，都会给被测元素的父元素记录 `ChildrenAffectedByLastChildRules`：

```cpp
case CSSSelector::PseudoClass::LastChild: {
    ...
    addStyleRelation(checkingContext, *parentElement, Style::Relation::ChildrenAffectedByLastChildRules);
```

`Source/WebCore/style/ChildChangeInvalidation.cpp`：子元素变动后，若父元素带有该标记且最后一个子元素变了，就让原最后一个子元素的整棵子树失效：

```cpp
static void invalidateForLastChildState(Element& child, bool state)
{
    auto* style = child.renderStyle();
    if (!style || style->lastChildState() == state)
        child.invalidateStyleForSubtree();
}

void ChildChangeInvalidation::checkForSiblingStyleChanges()
{
    ...
    if (parent->childrenAffectedByLastChildRules() && elementBeforeChange) {
        RefPtr newLastElement = ElementTraversal::lastChild(parent.get());
        if (newLastElement != elementBeforeChange)
            invalidateForLastChildState(*elementBeforeChange, true);
```

`:first-child` 是对称的：插入到第一个子元素之前时，原第一个子元素的子树失效。相邻兄弟组合器（`+`）走 `invalidateForSiblingCombinators`，只让后一个兄弟元素**自身**失效（`invalidateStyle()`），不涉及子树；追加到末尾时没有后续兄弟，不产生失效。

标记只在位置伪类真正被求值时才会记录。实测表明，组合器位于 `:where()` 之外时，祖先里没有对应类名的元素根本不会走到 `:last-child` 的判断；组合器被包进 `:where()` 后，这一提前排除失效（见 §3）。我们推断这是 WebKit 基于祖先标识的选择器过滤（SelectorFilter）所致，但没有逐行核对这部分源码，这一解释的可信度为中；实测结论本身的可信度为高。

## 3. 实测数据

环境：Playwright 无头 WebKit / Chromium，生产构建，加载应用完整的 Tailwind 样式表并先剥离其中所有位置伪类规则，再单独注入待测规则；DOM 为约 2000 个元素的子树，文档中没有带 `.zz` 类的元素，测量单次插入加上强制布局的耗时（5 次平均）。

| 选择器写法                                                 | 特异度 | WebKit 追加到末尾 | WebKit 插到开头 | Chromium          |
| ---------------------------------------------------------- | ------ | ----------------- | --------------- | ----------------- |
| 无                                                         | —      | 0                 | 0–0.2 ms        | ≤ 0.1 ms          |
| `:where(.zz > :not(:last-child))`（Tailwind v4 产物形状）  | 0      | **7.4–8.0 ms**    | 0–0.2 ms        | ≤ 0.1 ms          |
| `:where(.zz) > :where(:not(:last-child))`（方案 C 改写后） | 0      | 0                 | 0               | 0                 |
| `.zz > :not(:last-child)`                                  | 0,2,0  | 0                 | 0               | 0                 |
| `.zz > :where(:not(:last-child))`                          | 0,1,0  | 0                 | 0               | 0                 |
| `.cls:last-child`（主体带类名，如 `last:border-b-0`）      | 0,2,0  | 0.2 ms            | 0               | 0                 |
| `:where(.zz > * + *)`（lobotomized owl）                   | 0      | 0                 | 0.2 ms          | 插到开头约 2.0 ms |
| `:where(.zz > :not(:first-child))`                         | 0      | 0                 | **9.0 ms**      | 1.5 ms            |

规律：**只要位置伪类被包在带组合器的 `:where()` 里，WebKit 就会对每个元素求值并打标记**；组合器在 `:where()` 之外，或主体带类名时，代价可以忽略。

应用内的对照（WebKit，同一页面状态）：

| 探测                                              | 保留全部规则 | 剥离 `:last-child` 规则 |
| ------------------------------------------------- | ------------ | ----------------------- |
| 向 `body` 追加 `display:none` 的空 div            | 14 ms        | ≈ 0                     |
| 向 proxies 内容区（旧分组页面的父元素）追加空 div | 8 ms         | ≈ 0                     |
| 切换分组时虚拟列表 `offsetWidth` 触发的强制布局   | 13–17 ms     | 3 ms                    |
| 切换分组的最长一帧                                | ≈ 65 ms      | ≈ 55 ms                 |

剥离 `~` 规则（`peer-*`）没有可测影响；这些规则的主体带类名。

方案 C 实施后的切换分组（WebKit，30 组 × 1500 节点，两轮）：

| 指标     | 实施前   | 实施后   |
| -------- | -------- | -------- |
| 最长一帧 | 64–73 ms | 51–53 ms |
| 同步耗时 | 32–35 ms | 18–20 ms |

与剥离全部位置伪类规则的上限估计一致。

## 4. 方案评估

### 4.1 在 Tailwind 内替换实现：不可行

用项目的 Tailwind 4.3.3（`@tailwindcss/node` `compile`）验证：

- 同名 `@utility space-y-* { & > * + * { ... } }` **不会替换**内置实现，输出中两条规则并存。
- `@source not inline("space-y-2")` 会把该候选类整个排除，自定义实现也不再生成。

因此无法在不改类名的情况下替换 `space-*` / `divide-*` 的选择器。用自定义类代替 `flex flex-col gap-N` 也只能省去书写，无法减少方案 A 逐处核对视觉的工作量。

### 4.2 方案对比

| 方案                             | 做法                                                                                                                                              | 视觉风险                                                  | 结论                                     |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- | ---------------------------------------- |
| A. 修改标记                      | `space-y-N` → `flex flex-col gap-N`；`space-x-N` → `flex gap-N`；`divide-y` → 子元素 `border-b last:border-b-0`（或 `border-t first:border-t-0`） | 需逐处核对，见 §4.3                                       | 性能上已不需要；可在日常修改中按需迁移   |
| B. 构建时改写为 owl              | PostCSS 把 `> :not(:last-child)` 改成 `> * + *`，并把 margin 移到开始一侧                                                                         | 隐藏子元素、`-reverse` 变体、子元素自带 margin 时表现不同 | 不采用：语义不等价                       |
| C. 构建时拆分 `:where()`（采用） | PostCSS 在 Tailwind 之后把 `:where(A > B)` 改写为 `:where(A) > :where(B)`                                                                         | 无：两者特异度同为 0，匹配的元素相同                      | 采用；不改标记，约 50 处用法一次全部生效 |

### 4.3 方案 C 的实现

- 插件：`frontend/nyanpasu/postcss/split-where-combinators.js`，在 `postcss.config.js` 中位于 `@tailwindcss/postcss` 之后，基于 `postcss-selector-parser`。
- 只改写"整个选择器恰好是一个 `:where()`，且其中只有一个复杂选择器"的情况，这正是 Tailwind 的产物形状；在最后一个组合器处拆分。`:where()` 与其他简单选择器组合、含多个参数、或右侧含伪元素时保持不变。
- 生产构建中，恰好 13 条 `space-*` / `divide-*` 规则被改写，且只有选择器变化，其余 CSS 与改写前一致。
- 局限：收益依赖 WebKit 的实现细节，但改写在语义上等价，最坏情况只是没有收益。若将来 Tailwind 改变产物形状，插件会不再生效，由 §4.6 的守护测试发现。

### 4.4 方案 A 的视觉等价条件（供按需迁移时参考）

- 子元素为块级元素。原本是 inline 的子元素（`span`、`a`）放进 flex 后会变成块级，布局改变。
- 子元素没有自带的上下 margin。flex 中 margin 不折叠，原本会折叠的间距会变大。
- 最后一个子元素不是隐藏元素。
- flex 子元素默认 `min-height: auto`，可能影响 `truncate` 或滚动容器的收缩，必要时加 `min-h-0`。
- `space-x` 原为行内排列时，flex 不会自动换行，基线对齐也不同，可能需要 `flex-wrap items-baseline`。
- 直接写在容器里的文本节点会成为匿名 flex 子项。

### 4.5 影响面（截至 2026-09-30 的 `main`）

- 约 50 处 `space-*` / `divide-*`，集中在 settings、profiles、topology、connections、rules、logs 页面，以及 `animated-tabs.tsx`、`segmented-button.tsx`、`delay-history.tsx`。方案 C 一次覆盖全部用法。
- connections、rules、logs 的虚拟列表容器使用 `divide-y`。拆分后，只有这些容器（以及 `.divide-*` 祖先之下的元素）仍会记录 last-child 标记；向容器追加新行时，仍会让前一行的子树失效，但范围限于单行，属于 `divide-*` 语义本身的代价。
- `pages/(main)/main/settings/_modules/settings-card.tsx` 的 `[&>*:first-child:not(:only-child)]` 等任意变体，以及 `settings/user-interface/_modules/theme-color-config.tsx` 的 `[&>div:first-child]`：Tailwind 生成的规则没有 `:where()` 包裹，组合器在外层，不受影响，无需修改。

### 4.6 验证与防回归

- 守护测试：`frontend/nyanpasu/tests/split-where-combinators.test.ts` 用 `postcss.config.js` 编译应用样式表，若再次出现"组合器与位置伪类同在一个 `:where()` 内"的选择器即失败。移除插件后该测试会失败（已验证）。
- 性能：用 `pnpm -F @nyanpasu/nyanpasu bench` 在 `PERF_BROWSER=webkit` 下对比；`VITE_PERF_TRACE_REFLOW=1` 可定位强制重排。
- 视觉：方案 C 在语义上等价，无需截图比对。若按需实施方案 A，可用 vitest 5 浏览器模式的 `toMatchScreenshot`（见 `@vitest/browser` 类型定义）对受影响的页面做前后比对。

## 5. 样式性能工具调研

| 工具                                                               | 能力                                                                                                  | 对本问题                                                             |
| ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| Chrome / Edge DevTools → Performance → "Enable CSS selector stats" | 每次 Recalculate Style 中各选择器的耗时、尝试匹配次数、命中次数、慢路径比例；开启后本身会显著放大耗时 | **无法发现**：Blink 下该开销几乎为 0，且问题在于失效范围而非匹配速度 |
| Safari Web Inspector → Timelines → Layout & Rendering              | Styles Invalidated / Recalculated 事件的耗时与触发的 JS；较新版本可显示关联的 DOM 节点                | 能看到插入后样式重算很长，但没有按选择器拆分的统计                   |
| `frontend/nyanpasu/perf/` 基准                                     | WebKit / Chromium 自动化测帧，追踪强制重排，Chromium 下可导出 CPU profile                             | 本次即通过它定位                                                     |

可信度：Chrome / Edge 的 selector stats 有两个独立官方来源，高；Safari "关联 DOM 节点"只有单一来源，中。

## 6. Tailwind 官方资料

- 没有专门的运行时样式性能指南或工具（可信度：中；依据为检索结果与官方文档浏览）。
- 相关内容仅有：v4 升级指南的 "Space-between selector"、"Divide selector" 两节，说明为解决大页面性能问题而更换选择器，并建议改用 `flex` / `grid` + `gap`；以及对应的 PR 与讨论。
- 当初的改动针对的是 v3 旧写法 `> :not([hidden]) ~ :not([hidden])` 的匹配开销（PR 称快了近 2000 倍），没有涉及 WebKit 在插入元素时的失效范围。未检索到关于 v4 写法在 WebKit 下这一问题的报告（可信度：低，仅为未检索到）。
- `@tailwindcss/upgrade` 是版本迁移工具；Oxide 引擎优化的是构建速度，均与浏览器渲染无关。

## 7. 参考

- WebKit, 2026-09-30 读取, `Source/WebCore/css/SelectorChecker.cpp`, <https://github.com/WebKit/WebKit/blob/main/Source/WebCore/css/SelectorChecker.cpp>
- WebKit, 2026-09-30 读取, `Source/WebCore/style/ChildChangeInvalidation.cpp`, <https://github.com/WebKit/WebKit/blob/main/Source/WebCore/style/ChildChangeInvalidation.cpp>
- Tailwind Labs, v4 Upgrade Guide, "Space-between selector" / "Divide selector", <https://tailwindcss.com/docs/upgrade-guide#space-between-selector>
- Tailwind Labs, 2024-04, "Don't accommodate hidden elements in space/divide" #13459, <https://github.com/tailwindlabs/tailwindcss/pull/13459>
- Tailwind Labs, Discussion #13445, "Significant performance penalty when using 'space-' space between class", <https://github.com/tailwindlabs/tailwindcss/discussions/13445>
- Google, "Analyze CSS selector performance during Recalculate Style events", <https://developer.chrome.com/docs/devtools/performance/selector-stats>
- Microsoft, "Analyze CSS selector performance during Recalculate Style events", <https://learn.microsoft.com/microsoft-edge/devtools/performance/selector-stats>
- WebKit, "Timelines Tab", <https://webkit.org/web-inspector/timelines-tab/>
