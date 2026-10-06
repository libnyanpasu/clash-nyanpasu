# PR-5 bridge 前端：daemon 兼容状态可见化 + 核心状态徽章修复（2026-08-31）

- **基线**：`origin/main` = `3d5a518d`（含 #5070 fail-closed 兼容门 与 #5116 CoreActor v2/ServiceActor 未接线）。本计划所列的**全部目标文件与 `origin/main` 逐字节相同**（`git diff origin/main -- <各目标文件>` 为空），因此下文所有 `文件:行号` 引用对 `origin/main` 直接有效。
- **范围**：仅两件事——① 把 daemon 兼容状态渲染到用户管理服务模式的地方；② 修复 `widget-shortcut.tsx` 核心状态徽章的错误分支。不做重设计，不做额外 UX 打磨，不动 Rust。
- **前置**：无。当前 bindings 面已足够（见 §2 结论），**不阻塞于 app-switch 计划**。
- **交叉依赖**：并行的 `docs/superpowers/plans/2026-08-31-pr5-bridge-app-switch.md`（撰写中，本文写作时尚不存在）如果在其「前端影响面」章节给出更丰富的兼容面（例如 compat 事件推送、`update_service` 命令、或把 `ServicePhase` 暴露给前端），**实施者应优先采用该面**，并按 §7 调整。

---

## 1. 问题陈述（现状核实）

### 1.1 后端已 fail-closed，但前端完全沉默

`backend/tauri/src/core/service/compat.rs:13-27` 定义了四态 `ServiceCompat`：

| kind           | 语义                                        | `allows_service_backend()` |
| -------------- | ------------------------------------------- | -------------------------- |
| `unknown`      | daemon 未安装 / 未运行 / 未上报 server 信息 | false                      |
| `compatible`   | 主版本 == 2                                 | **true**                   |
| `incompatible` | 主版本 != 2（典型 v1.4.5）                  | false                      |
| `unparsable`   | server 上报的版本不是合法 semver            | false                      |

`compat.rs:11` 固定 `REQUIRED_SERVICE_MAJOR = 2`；`compat.rs:31-52` 的 `classify` 是纯函数；`compat.rs:55-57` 的 `allows_service_backend` 是唯一放行判据。

门禁的实际落点在 `backend/tauri/src/core/service/ipc.rs:194-202`：

```rust
pub(super) fn target_ipc_state(info: &StatusInfo<'_>) -> (IpcState, ServiceCompat) {
    let compat = ServiceCompat::classify(info);
    let state = match info.status {
        ServiceStatus::Running if compat.allows_service_backend() => IpcState::Connected,
        _ => IpcState::Disconnected,
    };
    (state, compat)
}
```

再经 `backend/tauri/src/core/clash/core.rs:51-62` 的 `RunType::classify(enable_service, ipc_state)`：`enable_service && ipc_state.is_connected()` 才是 `RunType::Service`，否则**静默退回 `RunType::Normal`**。

**结论**：用户把「服务模式」开关打开、daemon 装的是 v1，应用照常以子进程方式跑内核，界面上没有任何提示，开关看上去是「开着且生效」。这就是要修的洞。

### 1.2 兼容状态已经过 IPC 暴露，但界面没有任何消费点

- `backend/tauri/src/ipc.rs:923-933` 的 `status_service` 已在返回体上追加 `compat`。
- `frontend/interface/src/ipc/bindings.ts:2075-2083` 已生成完整的 `ServiceCompat` 判别联合（含 `server_version` / `required_major`）；`bindings.ts:2114-2120` 的 `ServiceStatusInfo_Serialize` 带 `compat: ServiceCompat`；`bindings.ts:224-227` 的 `commands.statusService()` 返回该类型。
- `frontend/interface/src/ipc/index.ts` 末尾 `export type * from './bindings'`，故 `ServiceCompat` 可直接从 `@nyanpasu/interface` 以类型导入。
- 全仓 grep：**`frontend/nyanpasu/` 下 `compat` 零命中**。两个消费 `useSystemService()` 的界面点都没读它：
  - `frontend/nyanpasu/src/pages/(main)/main/settings/system/_modules/system-service-switch.tsx:24` —— `const disabled = query.data?.status === 'not_installed'`，只看安装与否。
  - `frontend/nyanpasu/src/pages/(main)/main/settings/system/_modules/system-service-ctrl.tsx:283-293` —— 只渲染 Service Name / Server Version / Service Status 三行。

### 1.3 `widget-shortcut.tsx` 核心状态徽章：不是 TypeError，是两处死赋值

复核 `frontend/nyanpasu/src/pages/(main)/main/dashboard/_modules/widget-shortcut.tsx:136-174` 后，**修正上游 review 的结论**：

- 第 144-145 行 `serviceStatus?.server?.core_infos.state...` 用的是可选链，JS 可选链一旦短路会让**整条链**（含后续非可选属性访问）求值为 `undefined`，`server` 为 null 时不会抛 TypeError。
- 第 149 行 `serviceStatus?.server.core_infos.state.Stopped` 确实少了 `server` 后的 `?.`，但它被第 143-146 行的 `if` 守着（该条件为真蕴含 `server` 非空），**运行期不可达 null**，TS 也靠可选链真值收窄放行。

真正的缺陷是控制流：

1. **第 147-154 行的 `stopedMessage` 被第 165-171 行无条件覆盖** —— `dashboard_widget_core_stopped_by_service_with_message` 与 `..._unknown` 两条文案永远显示不出来。
2. **第 158-162 行的 `else` 吞掉了 `status === 'running'`** —— 第 140 行刚赋的 `dashboard_widget_core_service_running` 立刻被 `dashboard_widget_core_service_not_installed` 覆盖。于是 **daemon 正在运行但内核停着时，徽章会说「服务未安装」**，是错误信息。

修法见 §4 卡 F3：把两段 if/else 补全为互斥分支，顺便把 `state` 提成局部变量，第 149 行那处非对称可选链自然消失（这也满足「按周边约定守住 null」的诉求）。

---

## 2. 现有 bindings 面是否够用

够。渲染「装的是哪个版本 / 要求哪个主版本」所需的两个字段都在 `ServiceCompat` 的 `incompatible` 分支里（`server_version: string`、`required_major: number`），`unparsable` 分支带 `server_version`。**本计划不需要任何 Rust 改动，不构成对 app-switch 计划的排序依赖。**

已知的**面上限制**（如实记录，不在本计划内解决）：

- **无推送**：`bindings.ts:349-362` 的 `events` 里没有 service/compat 事件。`useSystemService`（`frontend/interface/src/ipc/use-system-service.ts:14-20`）是无 `refetchInterval` 的普通 `useQuery`，`frontend/interface/src/provider/index.tsx:32-56` 创建 `QueryClient` 时未改默认值，故刷新时机 = 挂载 + 窗口聚焦（react-query 默认 `refetchOnWindowFocus`）+ `upsert` 成功后的 `invalidateQueries`。用户在应用外升级了 daemon，需切回窗口才更新。**本计划不加轮询**（加轮询属于新行为，不在范围内）。
- **无「升级服务」命令**：`bindings.ts` 与 `backend/tauri/src/ipc.rs` 均无 `update_service`。补救路径只能是「卸载 → 安装」，文案按此写。
- `unknown` 无法区分「未安装」与「已安装但停止」，但 `status` 字段可以，界面按 `status` 判断即可。

---

## 3. 目标与成功判据

| #   | 判据                                                                                                                                                      | 验证方式                 |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| G1  | `compat.kind` 为 `incompatible` / `unparsable` 时，系统设置页「系统服务」卡片出现红色说明，写明**装到的版本**与**要求的主版本**，并给出卸载重装的补救指引 | 手工冒烟（§6）+ 代码走查 |
| G2  | 同上两态下，服务模式开关被禁用（当前值为关时），悬停有解释性 tooltip                                                                                      | 手工冒烟                 |
| G3  | 服务模式当前为**开**且 daemon 不兼容时，开关**仍可关闭**（不把用户锁死在无效的 ON），且 tooltip 仍解释原因                                                | 手工冒烟                 |
| G4  | `compat.kind` 为 `compatible` / `unknown` 时，界面与今日完全一致（零回归）                                                                                | 手工冒烟                 |
| G5  | 五个 locale 均有新键；`m.<新键>()` 编译通过                                                                                                               | `pnpm lint:ts:nyanpasu`  |
| G6  | daemon 运行、内核停止时，仪表盘徽章不再说「服务未安装」                                                                                                   | 手工冒烟 + 代码走查      |
| G7  | 类型 / lint / 格式 / 构建全绿                                                                                                                             | §5 命令序列              |

---

## 4. 任务卡

三张卡。**F1 + F2 合为一个提交**（同一件事的两半：文案与消费点，拆开则单独任一提交不可用）；**F3 独立提交**（无关缺陷）。

### 卡 F1 —— 新增 i18n 键（五 locale）

| 项       | 内容                                                                                                                                                                                                                                                                                                                                  |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 文件     | `frontend/nyanpasu/messages/{en,ko,ru,zh-cn,zh-tw}.json`                                                                                                                                                                                                                                                                              |
| 约定     | inlang message-format（`frontend/nyanpasu/project.inlang/settings.json`：`baseLocale: en`，`locales: [en, ko, ru, zh-cn, zh-tw]`，`pathPattern: ./messages/{locale}.json`）。参数写作 `{name}`，生成的入参类型是 `{ name: NonNullable<unknown> }`，数字可直接传（对照 `settings_clash_settings_mixed_port_label_value` 的生成产物）。 |
| 插入位置 | 五个文件的锚点行号一致：`settings_system_proxy_service_mode_disabled_tooltip` 在**第 41 行**、`settings_system_proxy_system_service_ctrl_stop` 在**第 54 行**。新 tooltip 键插在第 41 行之后；两条卡片文案插在第 54 行之后。                                                                                                          |
| 验证     | `pnpm lint:ts:nyanpasu`（须先按 §5 步骤 1 重新编译 paraglide，否则 `m.<新键>` 报 TS2339）                                                                                                                                                                                                                                             |

新增三个键（键名沿用既有前缀分组：开关归 `service_mode_*`，卡片归 `system_service_ctrl_*`）：

**`settings_system_proxy_service_mode_incompatible_tooltip`**（开关 tooltip，务必短——`TooltipContent` 是 `w-fit` 的 `rounded-full text-xs` 药丸，长句会很难看，详情放卡片）

| locale | 值                                                                    |
| ------ | --------------------------------------------------------------------- |
| en     | `The installed service is not compatible. See System Service below.`  |
| ko     | `설치된 서비스가 호환되지 않습니다. 아래 시스템 서비스를 확인하세요.` |
| ru     | `Установленная служба несовместима. См. «Системная служба» ниже.`     |
| zh-cn  | `已安装的服务不兼容，详见下方「系统服务」`                            |
| zh-tw  | `已安裝的服務不相容，詳見下方「系統服務」`                            |

**`settings_system_proxy_system_service_ctrl_incompatible`**（参数 `version`、`required`）

| locale | 值                                                                                                                                     |
| ------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| en     | `The installed service is v{version}, but service mode requires v{required}.x. Uninstall the service and install it again to upgrade.` |
| ko     | `설치된 서비스는 v{version}이지만 서비스 모드에는 v{required}.x가 필요합니다. 서비스를 제거한 후 다시 설치하여 업그레이드하세요.`      |
| ru     | `Установлена служба v{version}, но для режима службы требуется v{required}.x. Удалите службу и установите её заново, чтобы обновить.`  |
| zh-cn  | `已安装的服务为 v{version}，服务模式需要 v{required}.x。请先卸载服务再重新安装以升级。`                                                |
| zh-tw  | `已安裝的服務為 v{version}，服務模式需要 v{required}.x。請先解除安裝服務再重新安裝以升級。`                                            |

**`settings_system_proxy_system_service_ctrl_unparsable`**（参数 `version`；文案不使用引号字符，避免 JSON 转义）

| locale | 值                                                                                                                                             |
| ------ | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| en     | `The installed service reported an unrecognized version ({version}). Service mode stays disabled. Uninstall the service and install it again.` |
| ko     | `설치된 서비스가 인식할 수 없는 버전({version})을 보고했습니다. 서비스 모드는 비활성 상태로 유지됩니다. 서비스를 제거한 후 다시 설치하세요.`   |
| ru     | `Установленная служба сообщила о нераспознанной версии ({version}). Режим службы остаётся отключённым. Удалите службу и установите её заново.` |
| zh-cn  | `已安装的服务上报了无法识别的版本（{version}），服务模式保持禁用。请先卸载服务再重新安装。`                                                    |
| zh-tw  | `已安裝的服務回報了無法辨識的版本（{version}），服務模式維持停用。請先解除安裝服務再重新安裝。`                                                |

> 注：`ko.json` 当前比 `en.json` 少 4 个键（`deep_link_import_*`），说明仓库不强制 locale 全等；但本计划**五个 locale 全加**，不留缺口。

### 卡 F2 —— 消费兼容状态：开关门禁 + 卡片说明

| 项   | 内容                                                                                                                                                                                                     |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 文件 | `frontend/nyanpasu/src/pages/(main)/main/settings/system/_modules/system-service-switch.tsx`（改）、`.../system-service-ctrl.tsx`（改）、`frontend/nyanpasu/src/generated/data-slots.gen.ts`（重新生成） |
| 验证 | `pnpm lint:ts:nyanpasu` + `pnpm lint:oxlint` + `pnpm generate:data-slots` 后 `git diff` 只多一行 slot + §6 冒烟                                                                                          |

#### F2-a `system-service-switch.tsx`

把第 24 行的 `disabled` 派生替换为下述形状，并把第 64-70 行的 tooltip 条件改成按提示内容驱动。**不新增 import**（`m`、`useSystemService`、`useSetting` 已在第 8/11 行导入）。

```tsx
export default function SystemServiceSwitch() {
  const serviceMode = useSetting('enable_service_mode')

  const { query } = useSystemService()

  const notInstalled = query.data?.status === 'not_installed'

  // fail-closed 兼容门（backend/tauri/src/core/service/compat.rs）：这两态下
  // RunType::classify 永远退回 Normal，开关打开也不会走 Service backend。
  const compatKind = query.data?.compat.kind

  const compatBlocked =
    compatKind === 'incompatible' || compatKind === 'unparsable'

  // 不兼容时只拦「打开」，已开启的仍可关闭——否则用户会被锁死在一个无效的 ON 状态。
  const disabled = notInstalled || (compatBlocked && !serviceMode.value)

  const hint = compatBlocked
    ? m.settings_system_proxy_service_mode_incompatible_tooltip()
    : notInstalled
      ? m.settings_system_proxy_service_mode_disabled_tooltip()
      : null

  // handleServiceMode 不变（原第 26-38 行）
  ...
}
```

JSX 侧只改 tooltip 的渲染条件（原第 64-70 行）：

```tsx
{
  hint && (
    <TooltipContent>
      <span>{hint}</span>
    </TooltipContent>
  )
}
```

要点：

- `query.data?.compat.kind` 类型是 `ServiceCompat['kind'] | undefined`，四个成员都有 `kind`，无需 import 类型。
- `compatBlocked` 只取 `incompatible` / `unparsable`。**`unknown` 不参与门禁**——它同时覆盖「未安装」和「已安装但停止」，后者今天是允许打开的，改它就是行为回归（G4）。
- 数据加载中 / 查询失败时 `query.data` 为 `undefined`，`compatBlocked` 为 `false`，行为与今天一致。
- 保留 `data-slot="system-service-switch-container"` / `"system-service-switch-trigger-wrapper"`，不新增 slot。

#### F2-b `system-service-ctrl.tsx`

在第 91 行 `ServiceDetailButton` 之后（或第 57 行 `SystemServiceCtrlItem` 之后，位置不影响行为，取前者以贴近数据展示区）新增一个局部组件，并在 `SystemServiceCtrl` 的 `SettingsCardContent` 尾部挂载：

```tsx
const ServiceCompatWarning = () => {
  const { query } = useSystemService()

  const compat = query.data?.compat

  const warning =
    compat?.kind === 'incompatible'
      ? m.settings_system_proxy_system_service_ctrl_incompatible({
          version: compat.server_version,
          required: compat.required_major,
        })
      : compat?.kind === 'unparsable'
        ? m.settings_system_proxy_system_service_ctrl_unparsable({
            version: compat.server_version,
          })
        : null

  return (
    <AnimatePresence initial={false}>
      {warning && (
        <SettingsCardAnimatedItem
          className="text-error"
          data-slot="system-service-compat-warning"
        >
          {warning}
        </SettingsCardAnimatedItem>
      )}
    </AnimatePresence>
  )
}
```

挂载点（原第 282-294 行的 `SettingsCardContent` 内、三行 `SystemServiceCtrlItem` 之后）：

```tsx
<SettingsCardContent className="gap-2 py-4">
  <SystemServiceCtrlItem name="Service Name" value={query.data?.name} />

  <SystemServiceCtrlItem
    name="Server Version"
    value={query.data?.server?.version}
  />

  <SystemServiceCtrlItem
    name="Service Status"
    value={startCase(query.data?.status)}
  />

  <ServiceCompatWarning />
</SettingsCardContent>
```

新增两个 import（`pnpm exec prettier --write` 会按 `.prettierrc.cjs` 的 `importOrder` 自动排序，不必手工纠位）：

```tsx
import { AnimatePresence } from 'framer-motion'
```

以及把第 27-31 行既有的 settings-card 导入补上 `SettingsCardAnimatedItem`：

```tsx
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../_modules/settings-card'
```

要点与依据：

- 形状照抄本目录既有的行内错误条模式：`frontend/nyanpasu/src/pages/(main)/main/settings/system/_modules/proxy-guard-config.tsx:75-81`（`AnimatePresence initial={false}` + `SettingsCardAnimatedItem className="text-error"`）。`text-error` 对应 `frontend/nyanpasu/src/assets/styles/tailwind.css:49` 的 MD3 `--color-error`，是全仓统一的错误色（`proxy-bypass-config.tsx:78`、`mixed-port-config.tsx:141`、`core-secret-config.tsx:192` 同款）。
- `SettingsCardAnimatedItem` = `AnimatedItem`（`frontend/nyanpasu/src/pages/(main)/main/settings/_modules/settings-card.tsx:125` → `frontend/nyanpasu/src/components/ui/animated-item.tsx:5-36`），props 是 `ComponentProps<typeof motion.div>`，接受 `data-slot`。
- `SettingsCardContent` 底层是 `flex flex-col`（`frontend/nyanpasu/src/components/ui/card.tsx:24`），说明条自然堆在三行之下。
- **不新增按钮**：无 `update_service` 命令（§2），卡片页脚已有 Uninstall / Install，文案指向它们即可。
- **不改 `ServiceDetailButton`**：它 `JSON.stringify(query.data)`（第 80 行），`compat` 已自动出现在详情弹窗里。
- 新增了 `data-slot="system-service-compat-warning"`，必须跑 `pnpm generate:data-slots` 重生成 `frontend/nyanpasu/src/generated/data-slots.gen.ts`（该文件已入库，被 `frontend/nyanpasu/src/utils/custom-css-compiler.ts:13` 与 `.../utils/monaco-css.ts:10` 消费），并把它一并 add。预期 diff 只在 `'system-service-ctrl-item-value'` 与 `'system-service-container'` 之间插入一行。

### 卡 F3 —— `widget-shortcut.tsx` 核心状态徽章分支修复（独立提交）

| 项   | 内容                                                                                                                                                                                                                                                                                                                                      |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 文件 | `frontend/nyanpasu/src/pages/(main)/main/dashboard/_modules/widget-shortcut.tsx`                                                                                                                                                                                                                                                          |
| 范围 | 只替换 `CoreStatusBadge` 的 `useMemo` 中第 136-173 行；第 119-135、176-189 行不动；**不新增 i18n 键**（四条文案 `dashboard_widget_core_{service_running,service_stopped,service_not_installed,stopped_by_service_with_message,stopped_by_service_unknown,stopped_with_message,stopped_unknown}` 都已存在，见 `messages/en.json:328-334`） |
| 验证 | `pnpm lint:ts:nyanpasu` + `pnpm lint:oxlint` + §6 冒烟                                                                                                                                                                                                                                                                                    |

替换后（第 136 行起至 `return` 前）：

```tsx
let serviceMessage

if (serviceStatus?.status === 'running') {
  serviceMessage = m.dashboard_widget_core_service_running()
} else if (serviceStatus?.status === 'stopped') {
  serviceMessage = m.dashboard_widget_core_service_stopped()
} else {
  serviceMessage = m.dashboard_widget_core_service_not_installed()
}

let stopedMessage

// 先取 service 上报的内核状态；server 为空时整条链短路成 undefined，
// 后续访问都走这个局部变量，不再有非对称可选链。
const serviceCoreState = serviceStatus?.server?.core_infos.state

if (
  serviceStatus?.status === 'running' &&
  serviceCoreState !== undefined &&
  serviceCoreState !== 'Running'
) {
  // service 明确报告了内核已停：这一分支优先于本地 core status。
  stopedMessage = serviceCoreState.Stopped
    ? m.dashboard_widget_core_stopped_by_service_with_message({
        message: serviceCoreState.Stopped,
      })
    : m.dashboard_widget_core_stopped_by_service_unknown()
} else if (coreStatus?.status.Stopped) {
  stopedMessage = m.dashboard_widget_core_stopped_with_message({
    message: coreStatus.status.Stopped,
  })
} else {
  stopedMessage = m.dashboard_widget_core_stopped_unknown()
}

return `${stopedMessage} ${serviceMessage}`
```

修复了什么：

1. `serviceMessage` 三分支互斥，`running` 不再被 `else` 覆盖 → daemon 运行时不再误报「服务未安装」（G6）。
2. `stopedMessage` 的 service 分支与本地分支改成 `else if`，`stopped_by_service_*` 两条文案第一次真正可达。
3. `serviceCoreState` 局部变量取代原第 144/145/149 行三处重复链，其中第 149 行 `serviceStatus?.server.core_infos...`（`server` 后缺 `?.`）随之消失。

类型说明（`strict: true`）：`serviceCoreState` 是 `CoreState | undefined`（`bindings.ts:677` = `'Running' | { Stopped: string | null }`）；经 `!== undefined && !== 'Running'` 收窄为 `{ Stopped: string | null }`；`serviceCoreState.Stopped` 真值收窄为 `string`。`coreStatus?.status` 在第 128-134 行的 `if/return` 之后已排除 `'Running'`，`.Stopped` 访问与今日一致。

保留既有拼写 `stopedMessage`（AGENTS §3：匹配现有风格，不顺手改名）。

---

## 5. 验证命令序列（按顺序执行；全部在仓库根）

前置事实：

- **不需要 `pnpm -F interface build`**。`frontend/nyanpasu/tsconfig.json` 的 `paths` 把 `@nyanpasu/interface` 映到 `../interface/src/index.ts`，`frontend/nyanpasu/vite.config.ts:113-116` 的 alias 同样指向 `../interface/src`。typecheck 与 vite 构建都不读 `frontend/interface/dist`。（`pnpm -F @nyanpasu/interface build` 跑一遍无害，但不是本计划的前置。）
- **必须先重编译 paraglide**。`frontend/nyanpasu/src/paraglide/` 由 `frontend/nyanpasu/vite.config.ts:101-105` 的 `paraglideVitePlugin` 生成且被 gitignore（`frontend/nyanpasu/src/paraglide/.gitignore`），但它落在 `tsconfig.json` 的 `include: ["src", ...]` 里。加了 `messages/*.json` 却不重编译，`m.<新键>` 会以 TS2339 失败。

```bash
# 1. 重编译 paraglide（改了 messages/*.json 后必做）
pnpm -F @nyanpasu/nyanpasu exec paraglide-js compile \
  --project ./project.inlang --outdir ./src/paraglide --strategy custom-extension
#    strategy 值与 vite.config.ts:104 一致；custom-extension 满足 CLI 的
#    ^custom-[A-Za-z0-9_-]+$ 校验。跑过一次 pnpm web:dev / web:build 同样会重生成。

# 2. 重生成 data-slot 清单（仅当 F2 新增了 data-slot 时；需要 deno）
pnpm generate:data-slots

# 3. 类型检查
pnpm lint:ts:nyanpasu
pnpm lint:ts:interface        # interface 未改动，作为回归护栏

# 4. lint + 格式（surgical：只写触碰到的文件，避免全仓 reformat 噪声）
pnpm lint:oxlint
pnpm exec prettier --write \
  "frontend/nyanpasu/messages/*.json" \
  "frontend/nyanpasu/src/pages/(main)/main/settings/system/_modules/system-service-switch.tsx" \
  "frontend/nyanpasu/src/pages/(main)/main/settings/system/_modules/system-service-ctrl.tsx" \
  "frontend/nyanpasu/src/pages/(main)/main/dashboard/_modules/widget-shortcut.tsx"
pnpm lint:prettier

# 5. 端到端构建（同时二次验证 paraglide 生成正确）
pnpm web:build
#    注意副作用：vite.config.ts:160-162 的 outDir=../../backend/tauri/tmp/dist 且
#    emptyOutDir: true —— 会清空并重建该目录。主检出里这是期望行为。
```

**无前端单元测试**：仓库根 `package.json` 的 `test:*` 只有 `test:architecture-ledger`（deno）和 `test:backend`（cargo）；`frontend/` 下无 vitest/jest 配置、无 `*.test.ts(x)`。本计划因此以 typecheck + lint + 手工冒烟为判据，**不引入测试框架**（属范围外）。

---

## 6. 手工冒烟清单

需在装有 nyanpasu-service 的环境跑（`pnpm dev` / `pnpm tauri:dev`）。四个 `compat` 态里，`compatible` / `unknown` 是自然可达的，`incompatible` 需要装一个 v1 daemon（如 v1.4.5），`unparsable` 实际环境难构造。

| 场景                                                 | 期望                                                                                   |
| ---------------------------------------------------- | -------------------------------------------------------------------------------------- |
| daemon 未安装（`unknown` + `not_installed`）         | 开关禁用，tooltip = 既有的 `..._disabled_tooltip`；卡片无红色说明（**零回归**）        |
| daemon 已装已停（`unknown` + `stopped`）             | 开关可用，无 tooltip；卡片无红色说明（**零回归**）                                     |
| daemon v2 运行（`compatible`）                       | 开关可用，无 tooltip；卡片无红色说明（**零回归**）                                     |
| daemon v1.4.5 运行、服务模式**关**（`incompatible`） | 开关禁用；tooltip = 不兼容提示；卡片红字写出 `v1.4.5` 与 `v2.x`，指引卸载重装（G1/G2） |
| 同上但服务模式**开**                                 | 开关**仍可点，可关**；tooltip 仍是不兼容提示；卡片红字同上（G3）                       |
| daemon 运行、内核停止                                | 仪表盘 Core Status 徽章说「Service Running」，**不是**「Service Not Installed」（G6）  |
| 「服务详情」弹窗                                     | JSON 里能看到 `compat` 字段（无需改代码）                                              |

`unparsable` 无法自然构造时：在 `ServiceCompatWarning` 里临时把 `compat?.kind === 'incompatible'` 改成 `=== 'unparsable'` 走查一次渲染，**核对后回滚**，不入提交。

---

## 7. 与 app-switch 计划的交接

本计划按 **当前** bindings 面撰写，自洽可执行。若 `docs/superpowers/plans/2026-08-31-pr5-bridge-app-switch.md` 的「前端影响面」章节提供了更丰富的面，实施者应优先采用，具体替换点：

| 若 app-switch 提供                                          | 本计划的替换动作                                                                                                                                                      |
| ----------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| compat / ServicePhase 的 Tauri **事件**                     | 在 `frontend/interface/src/ipc/use-system-service.ts` 里订阅并 `invalidateQueries(['system-service'])`，替代「靠窗口聚焦刷新」（§2 的限制随之解除）。本计划不主动做。 |
| `ServicePhase`（`Incompatible` / `Exhausted` 等）暴露到 DTO | F2-a 的 `compatBlocked` 改为按 phase 判定；文案键名不变                                                                                                               |
| `update_service` / `upgrade_service` 命令                   | F1 的两条卡片文案改写为「点击升级」，F2-b 卡片页脚加一个升级按钮                                                                                                      |
| `status_service` 返回体字段重命名                           | 同步 F2-a / F2-b 的字段访问；键名与文案不变                                                                                                                           |

反向：**本计划不改任何 command / event 面**，对 app-switch 无输出依赖。

---

## 8. 提交纪律（AGENTS §18）

两个提交，各自完整可构建，显式路径 add，禁止 `git add .` / `-A`。

**提交 1**（卡 F1 + F2）

```bash
git add frontend/nyanpasu/messages/en.json \
        frontend/nyanpasu/messages/ko.json \
        frontend/nyanpasu/messages/ru.json \
        frontend/nyanpasu/messages/zh-cn.json \
        frontend/nyanpasu/messages/zh-tw.json \
        "frontend/nyanpasu/src/pages/(main)/main/settings/system/_modules/system-service-switch.tsx" \
        "frontend/nyanpasu/src/pages/(main)/main/settings/system/_modules/system-service-ctrl.tsx" \
        frontend/nyanpasu/src/generated/data-slots.gen.ts
git diff --cached --stat
```

subject：`feat(ui): surface the daemon compat gate in system settings`

body 要点：#5070 的兼容门在 `RunType::classify` 里静默退回子进程模式，界面没有任何信号，用户看到的是一个打开却不生效的服务模式开关；现在把 `status_service` 已经返回的 `compat` 渲染出来，并只拦「打开」动作、保留「关闭」通路。

**提交 2**（卡 F3）

```bash
git add "frontend/nyanpasu/src/pages/(main)/main/dashboard/_modules/widget-shortcut.tsx"
git diff --cached --stat
```

subject：`fix(ui): stop the core status badge from reporting a wrong service state`

body 要点：`serviceMessage` 的 `else` 分支吞掉了 `status === 'running'`，`stopedMessage` 的 service 分支被后续赋值无条件覆盖 —— daemon 正在运行、内核停止时徽章会说「服务未安装」，且两条 `stopped_by_service_*` 文案从未可达。

> 若实施中发现提交 1 需要补丁式修正且尚未 push，按 AGENTS §18 用 `git reset --soft HEAD~1` 折叠重提，不追加 fixup 提交。

---

## 9. 待用户裁定

两项，均已给出默认取值，可直接照做；仅在用户反对时改。

1. **不兼容且服务模式当前为「开」时，开关是否仍可操作？**
   默认：**可以**（只拦「打开」，允许「关闭」）。理由：不兼容时 `RunType` 已退回 `Normal`，这个 ON 值不产生任何效果；若一并禁用，用户将无法把它关掉，等于新增一个死锁。代价是这一态下开关本身没有 `disabled` 视觉，但 tooltip 与卡片红字都在解释原因。
   备选：无条件禁用（与 `not_installed` 一致，实现更简单，但制造上述死锁）。

2. **仪表盘 Core Status 徽章是否也提示不兼容？**
   默认：**不提示**。理由：徽章是单行跑马灯（`TextMarquee`），空间有限；设置页已给出完整解释与补救路径；扩到仪表盘属于「额外 UX 打磨」，被本任务范围明确排除。
   备选：在 F3 的 `serviceMessage` 分支里为 `compat.kind === 'incompatible'` 追加一条短文案（需新增 1 个 i18n 键 × 5 locale）。
