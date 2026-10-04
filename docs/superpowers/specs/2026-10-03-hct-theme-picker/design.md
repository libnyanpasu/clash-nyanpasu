# HCT 主题颜色选择器与实时预览 设计

## 1. 背景与目标

设置 → 用户界面 → 主题颜色目前是一个下拉菜单：5 个预设色、系统强调色，以及"自定义"子菜单里的一条 `@uiw/react-color` Hue 滑条（固定饱和度与亮度，打开时总是从 hue 0 开始，点"提交"保存）。它只能选纯色相，无法在当前颜色上微调，也不能输入颜色值。子菜单里的滑条还会被 Radix Menu 的键盘处理干扰：`ArrowLeft` 会关闭子菜单。

目标：参照 material-web catalog 的 Theme Controls（`catalog/src/components/theme-changer.ts`、`hct-slider.ts`），把主题颜色入口改成一个 Popover 面板：

- Hex Source Color 行实时显示并可直接编辑 hex 值；
- Hue（0–360）、Chroma（0–150）、Tone（0–100）三条滑条，每条下方是渐变预览；
- 拖动、输入、点预设时整个应用实时预览，点"应用"才写入后端；
- 颜色模式（深色 / 跟随系统 / 浅色）可在面板中切换，只供预览，任何情况下都不在这里保存。

HCT 选择器作为可复用的 shadcn 风格组合组件放在 `@nyanpasu/ui`。

## 2. 已确认的决策

1. **承载方式：Popover（方案 A）。** 点"主题颜色"设置行弹出面板，替换现有下拉菜单。Popover 非模态、无遮罩，背后的页面能直接看到预览效果。
2. **预设色与系统强调色移入面板**，点击只更新草稿并预览，点"应用"才保存（现在点预设会立即保存，这一行为改变）。
3. **"应用"只保存颜色。** 颜色模式的预览保持到面板关闭，关闭后恢复已保存的模式。
4. **关闭即放弃。** 点外部、按 Esc、离开页面时，未应用的颜色与模式预览全部撤销。
5. **色块只做展示**，不接原生 `<input type=color>`；自定义颜色值通过 hex 文本输入。
6. **依赖：** `@nyanpasu/ui` 直接依赖 `@material/material-color-utilities@0.4.0`（与 `@nyanpasu/theme`、app 同版本，lockfile 已有）。包边界规则允许，HCT 换算只需 `Hct.from` / `Hct.fromInt` 两个调用，不为此引入 ui → theme 的包间依赖。

## 3. UI 组件：`@nyanpasu/ui/hct-color-picker`

文件：`frontend/ui/src/hct-color-picker.tsx`，并在 `frontend/ui/src/index.ts` 中 `export * from './hct-color-picker'`。

### 3.1 组合方式

```tsx
<HctColorPicker value={draft} onValueChange={setDraft}>
  <HctColorPickerPresets aria-label={...}>
    <HctColorPickerPreset value="#9e1e67" />
    <HctColorPickerPreset value={systemAccentColor} label={...} />
  </HctColorPickerPresets>

  <HctColorPickerHexField label="Hex Source Color" />

  <HctColorPickerSliders>
    <HctColorPickerSlider channel="hue" label="Hue" />
    <HctColorPickerSlider channel="chroma" label="Chroma" />
    <HctColorPickerSlider channel="tone" label="Tone" />
  </HctColorPickerSliders>
</HctColorPicker>
```

所有可见文案通过 props 传入（ui 包不依赖 Paraglide）。每个部件都在 DOM 根上放 `data-slot`（写在 `{...props}` 之前，与 `settings-card` 一致），并转发 `className` 与其余 DOM props。受控 `value` 不是合法 hex 时按 `#000000` 处理：

| 部件                     | data-slot                                                                                                                                                                        |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `HctColorPicker`         | `hct-color-picker`                                                                                                                                                               |
| `HctColorPickerPresets`  | `hct-color-picker-presets`                                                                                                                                                       |
| `HctColorPickerPreset`   | `hct-color-picker-preset`                                                                                                                                                        |
| `HctColorPickerHexField` | `hct-color-picker-hex-field`，内部 `hct-color-picker-hex-label`、`hct-color-picker-hex-input`、`hct-color-picker-swatch`                                                         |
| `HctColorPickerSliders`  | `hct-color-picker-sliders`                                                                                                                                                       |
| `HctColorPickerSlider`   | `hct-color-picker-slider`，内部 `hct-color-picker-slider-label`、`-root`、`-track`、`-range`、`-thumb`、`hct-color-picker-slider-value`（数值气泡）、`hct-color-picker-gradient` |

### 3.2 根组件与 HCT 状态

`HctColorPicker` 的 props：`value?: string`、`defaultValue?: string`、`onValueChange?: (hex: string) => void`，以及 `div` 的其余 props。用 `useControllableState` 支持受控与非受控。通过 context 向部件提供当前 hex、当前 HCT 与设置函数。

**核心不变式：组件内部单独保存 `{ hue, chroma, tone }`，不在每次渲染时从 hex 反推。** 原因：

- HCT 色域大于 sRGB，`Hct.from(h, c, t).toInt()` 会把超出色域的 chroma 裁剪掉，从裁剪后的 hex 反推出的 chroma 比滑条值小，滑条会跳回去；
- chroma 接近 0 时 hue 没有定义，反推出的 hue 会跳到任意值。

material-web 的 `theme-changer` 也是把 hue / chroma / tone 作为独立状态保存。具体规则：

1. 初始化：从初始 hex 得到 HCT。
2. 组件记住"最近一次由自己发出的 hex"（规范化后）。外部 `value` 变化且不等于这个值时，从新 hex 重新得到 HCT，并把它记为最近值。
3. 从 hex 得到 HCT 时，若 chroma < 3（无彩色），保留当前的 hue，只更新 chroma 与 tone。阈值取 3 是因为 sRGB 纯灰在 HCT 中的 chroma 并不为 0，最高约 2.87（`#ffffff`），`#808080` 约 1.90。
4. 滑条改变某个通道：用新的三元组算出 hex（`hexFromArgb(Hct.from(h, c, t).toInt())`），更新内部 HCT，记为最近值并调用 `onValueChange(hex)`。
5. hex 输入或预设：规范化 hex → 按规则 3 更新 HCT → 记为最近值 → `onValueChange(hex)`。

输出的 hex 一律是小写 6 位 `#rrggbb`（`hexFromArgb` 的格式）。

### 3.3 Hex 解析

导出纯函数 `parseHexColor(input: string): string | null`：去掉首尾空白与可选的 `#`，接受 3 位或 6 位十六进制（大小写不限），3 位展开为 6 位，返回小写 `#rrggbb`；其他输入返回 `null`。不接受 8 位（MCU 的 `argbFromHex` 把 8 位按 ARGB 解析，和 CSS 的 RRGGBBAA 冲突）。

### 3.4 `HctColorPickerHexField`

参照 material-web 的 Hex Source Color 行：容器 `bg-surface-variant text-on-surface-variant rounded-3xl`，左侧上方是标签，下方是 hex 文本输入（等宽字体，透明背景，聚焦时显示 `primary` 下划线）；右侧是 48px 圆形色块（`border-outline` 描边，背景为当前 hex）。

- 未聚焦时，输入框显示当前 hex，拖动滑条时实时变化。
- 聚焦期间，外部值变化不覆盖用户正在输入的文本。
- 每次输入都用 `parseHexColor` 校验：合法时立即按 §3.2 规则 5 生效（从而实时预览），非法时设置 `aria-invalid="true"` 并显示 `error` 色，不发出值。
- 失焦或按 Enter：把文本恢复成当前（规范化后的）hex。
- 输入框的可访问名称来自标签（`<label htmlFor>` 或 `aria-labelledby`）。

### 3.5 `HctColorPickerSlider`

参照 material-web 的 `md-slider`（catalog 的 `hct-slider`）：部件直接基于 Radix Slider 原语实现，不复用 `@nyanpasu/ui` 的共享 `Slider`。自上而下为标签、滑条、渐变条（高 24px、`rounded-full`、1px `currentColor` 描边）。

- 滑条根高 40px；轨道高 4px、`rounded-full`，未激活段为 `on-surface-variant`，激活段为 `primary`；手柄为 20px 的 `primary` 圆（`shadow-sm`）。
- 手柄周围是 40px 的 `primary` 状态层（光晕），静止时透明度 0，悬停 8%，聚焦或按下 12%。
- 手柄上方有 `primary` 的"冰淇淋筒"数值气泡（取整的当前值，`aria-hidden`；手柄已通过 `aria-valuenow` 暴露数值），仅在手柄悬停、聚焦或按下时缩放显示（100ms，`cubic-bezier(0.2, 0, 0, 1)`）。因此不再在标签行右侧显示数值。
- 对齐：Radix 按手柄自身尺寸限制位置，20px 手柄的中心在 10px 到 width - 10px 之间移动。标签、轨道与渐变条各缩进 `mx-2.5`（10px），使手柄中心正好落在当前值对应的渐变色上（material-web catalog 自身有 10px 偏差，这里精确对齐）。

| channel  | 范围  | 渐变（100 个色标，`i` 为 0–99）                  |
| -------- | ----- | ------------------------------------------------ |
| `hue`    | 0–360 | `hexFromHct(3.6 * i, 100, 50)`，与当前颜色无关   |
| `chroma` | 0–150 | `hexFromHct(当前 hue, 1.5 * i, 50)`，随 hue 变化 |
| `tone`   | 0–100 | `hexFromHct(0, 0, i)`，灰阶，与当前颜色无关      |

渐变在部件内用 `useMemo` 计算，依赖为 channel 与"chroma 用的 hue"（hue / tone 通道恒为 0，所以只算一次；chroma 通道随 hue 重算），不引入模块级可变缓存。手柄（`role=slider`）的可访问名称为 `label`（`aria-label`）。

`HctColorPickerSliders` 是三条滑条的容器卡片（`bg-surface-variant rounded-3xl`，内边距与间距参照 material-web）。

### 3.6 预设

`HctColorPickerPresets` 是 `role="radiogroup"` 的横向色块行；`HctColorPickerPreset`（props：`value: string`、`label?: string`）是 32px 圆形按钮，`role="radio"`，`aria-checked` 为"当前 hex 与 `parseHexColor(value)` 相等"，选中时显示勾。可访问名称为 `label ?? value`。点击按 §3.2 规则 5 生效。`value` 不合法时不渲染。

## 4. 预览：`ExperimentalThemeProvider`

文件：`frontend/nyanpasu/src/components/providers/theme-provider.tsx`。

### 4.1 接口

```ts
export type ThemePreview = {
  color?: string
  mode?: ThemeMode
}

// context 新增
setThemePreview: (preview: ThemePreview | null) => void
```

- 生效颜色 = `preview?.color ?? 已保存的 theme_color`；生效模式 = `preview?.mode ?? 已保存的 theme_mode`。
- CSS 变量（`custom-theme` 样式）、`applyRootStyleVar`、html 与 `#root` 的 dark/light class、system 模式的系统主题监听，全部按生效值计算。
- `themeColor`、`themeMode` 仍返回**已保存**的值（设置行、顶栏模式菜单显示的是已保存状态）；`themePalette`、`themeCssVars`、`currentThemeMode` 返回**生效**的值。
- `setThemePreview` 是稳定引用（`useCallback`），传入与当前相同的预览不触发更新。
- 生效颜色经过 `useDeferredValue` 后再 `createTheme`，让拖动产生的高频更新由 React 合并，不阻塞输入。

### 4.2 缓存规则

`theme-palette-v1` / `theme-css-vars-v1` 是"下次启动在设置加载前先绘制的主题"。**预览颜色生效期间不写入缓存**，否则预览中途关闭应用，下次启动会先闪出预览色。清除预览后，主题按已保存颜色重新计算，缓存照常写入。

## 5. 面板：`theme-color-config.tsx`

文件：`frontend/nyanpasu/src/pages/(main)/main/settings/user-interface/_modules/theme-color-config.tsx`。删除 `hueToHex`、`@uiw/react-color` 引用与 `DropdownMenu` 结构，并从 `frontend/nyanpasu/package.json` 删除已无引用的 `@uiw/react-color`。

### 5.1 结构

触发器保持现有设置行（标签 + 已保存颜色的小圆点 + hex + 箭头图标），由 `DropdownMenuTrigger` 换成 `PopoverTrigger`。`PopoverContent` 从上到下：

1. `HctColorPickerPresets`：5 个预设色 + 系统强调色（有值时）；
2. `HctColorPickerHexField`；
3. `HctColorPickerSliders`（Hue / Chroma / Tone）；
4. 颜色模式 `SegmentedButton`（深色 / 跟随系统 / 浅色，顺序同 material-web；图标为 Material Symbols 的 dark mode / brightness medium / light mode；每项的 `aria-label` 与 `title` 复用 `settings_user_interface_theme_mode_*` 文案；组的 `aria-label` 复用 `settings_user_interface_theme_mode_label`）；
5. "应用"按钮（`common_apply`）。

面板宽度约 320px（Popover 已限制 `maxWidth: calc(100vw - 24px)`），各部分间距 12px。

定位：在设置行右下角放一个零尺寸的 `PopoverAnchor`（卡片 `relative`，锚点 `absolute right-0 bottom-0`），面板用 `side="left"`、`align="start"`、`sideOffset={0}`、`alignOffset={8}`、`sticky="always"`。面板右缘与卡片右缘对齐，顶部在设置行下方 8px。左侧放置时对齐轴是垂直方向，Radix 的 `shift` 会在面板越过窗口底部（12px 碰撞内边距）时整体上滑（可以盖住上方的行），Floating UI 的 `size` 在 y 轴 shift 启用时报告整个窗口高度（减去内边距）为可用高度，`sticky="always"` 去掉 `limitShift`，面板才能越过锚点。因此只有面板高于窗口高度减去两侧碰撞内边距时才滚动，而不是被限制在设置行下方的剩余空间内。

### 5.2 状态与流程

组件状态：`open`、`draft`（hex）、`previewMode`（`ThemeMode | undefined`）、`applying`（upsert 进行中）、`closeWhenSaved`（upsert 已成功，等已保存值追上）。

- **打开：** `draft` = 已保存颜色，`previewMode` = `undefined`。模式按钮显示 `previewMode ?? 已保存模式`。
- **预览：** 一个 effect 在 `open` 为真时调用 `setThemePreview({ color: draft, mode: previewMode })`，为假时调用 `setThemePreview(null)`；组件卸载（例如离开设置页）时也调用 `setThemePreview(null)`。
- **切换模式：** 只更新 `previewMode`，永远不调用 `setThemeMode`。
- **应用：** 当草稿与已保存颜色相同（忽略大小写）或 `applying` 时禁用。点击后置 `applying`，`await setThemeColor(draft)`：
  - 成功：结束 `applying`，置 `closeWhenSaved`。`useSetting` 的 `upsert` 只触发 invalidate，不等 refetch，resolve 时已保存值可能还是旧的；一个 effect 在 `closeWhenSaved` 且已保存颜色等于草稿（忽略大小写）时关闭面板，避免清除预览时先闪回旧色。
  - 失败：结束 `applying`，保留面板与预览，按设置页现有方式用 `message(..., { title: 'Error', kind: 'error', error })` 报错。
- **关闭：** 点外部、Esc、或应用成功且已保存值追上后关闭；关闭即清除预览，并复位 `closeWhenSaved`。只有 `applying` 期间的关闭请求被忽略（避免请求完成前预览被撤销）；upsert resolve 之后用户随时可以手动关闭，refetch 失败也不会把面板卡住。
- 设置行的"已选中"判断与显示一律按忽略大小写比较（系统强调色是大写 `#RRGGBB`，草稿是小写）。

### 5.3 i18n

在 5 个语言文件中，把下列 key 放在现有 `settings_user_interface_theme_color_*` 一组里（顺序一致）：

| key                                              | en               | zh-cn      | zh-tw        | ko            | ru                  |
| ------------------------------------------------ | ---------------- | ---------- | ------------ | ------------- | ------------------- |
| `settings_user_interface_theme_color_presets`    | Presets          | 预设       | 預設         | 프리셋        | Пресеты             |
| `settings_user_interface_theme_color_hex_source` | Hex Source Color | Hex 源颜色 | Hex 來源顏色 | Hex 소스 색상 | Исходный цвет (Hex) |
| `settings_user_interface_theme_color_hue`        | Hue              | 色相       | 色相         | 색조          | Оттенок             |
| `settings_user_interface_theme_color_chroma`     | Chroma           | 色度       | 彩度         | 채도          | Насыщенность        |
| `settings_user_interface_theme_color_tone`       | Tone             | 色调       | 色調         | 명도          | Тон                 |

`settings_user_interface_theme_color_custom` 不再使用，从所有语言文件删除。Paraglide 输出由 Vite 插件生成，不手改。

### 5.4 data-slot

保留 `theme-color-config-card` 与 `theme-color-config-colorful-preview`；删除只属于旧下拉菜单的 `theme-color-config-colorful-select-preview`、`theme-color-config-colorful-system-accent-preview`、`theme-color-config-colorful-custom-preview`；修正触发器误用的 `theme-mode-selector-trigger` 为 `theme-color-config-trigger`；模式按钮组为 `theme-color-config-mode-preview`，应用按钮为 `theme-color-config-apply`。面板主体直接用 `HctColorPicker` 的根（`hct-color-picker`），不为 slot 额外包一层 DOM（`PopoverSurface` 会用 `popover-content` 覆盖传入的 slot）。改完运行 `deno task generate:data-slots`。

`SegmentedButton`（Radix ToggleGroup single）再次点击已选项会发出空字符串，面板忽略空值，模式按钮始终保持一项选中。

## 6. 测试

- **`frontend/ui/tests/hct-color-picker.browser.test.tsx`：**
  - `parseHexColor` 的合法 / 非法 / 3 位展开 / 大小写 / 8 位拒绝（放在浏览器测试里，因为模块是 TSX 组件文件，unit 项目没有 React 插件）；
  - 拖动某个滑条后，`onValueChange` 收到的 hex 等于 `Hct.from(h, c, t)` 的结果；
  - 把 chroma 设到超出色域的值（例如 150）后，chroma 滑条的值保持 150，不回跳；
  - 在 hex 输入框输入合法值时发出规范化 hex 并同步三条滑条；输入非法值时不发出、`aria-invalid="true"`，失焦后恢复；
  - 输入框聚焦期间外部值变化不覆盖输入；
  - 受控 `value` 从外部改变时滑条重新同步；无彩色输入保留原 hue；
  - 点预设发出该颜色，`aria-checked` 正确。
- **`frontend/nyanpasu/tests/theme-provider.browser.test.tsx`（扩展）：**
  - 设置预览颜色后 `custom-theme` 样式改变，不调用 upsert，不写 localStorage；清除预览后恢复已保存颜色的样式；
  - 设置预览模式后 html 的 dark/light class 改变，不调用 upsert；清除后恢复；
  - 现有两个测试保持通过（稳定性：重复渲染不重建主题、不写缓存）。
- **`frontend/nyanpasu/tests/theme-color-config.browser.test.tsx`（新增）：**
  - 打开面板，拖动滑条 / 输入 hex / 点预设后预览生效但不调用 `theme_color` 的 upsert；
  - 切换模式不调用 `theme_mode` 的 upsert；
  - 点"应用"调用一次 `theme_color` upsert（小写 hex），已保存值更新后面板关闭；
  - 不应用直接关闭（Esc）后预览清除；
  - upsert 失败时面板保持打开并报错。
- **检查：** `pnpm typecheck`、`pnpm lint`、`pnpm test:frontend`（unit + browser），WebKit 预览截图检查浅色 / 深色下的面板。

## 7. 提交划分

| 提交 | 内容                                                                                     |
| ---- | ---------------------------------------------------------------------------------------- |
| C0   | `docs(theme): design the HCT theme picker with live preview`（本设计与任务清单）         |
| C1   | `feat(ui): add an HCT color picker`（§3 与对应测试、ui 包依赖）                          |
| C2   | `feat(settings): preview theme color and mode live before applying`（§4、§5 与对应测试） |

C2 中 provider 的预览接口与面板是同一个功能的两半，单独提交 provider 会留下无调用方的接口，所以放在一起。
