# PR-3 T09 — 前端适配 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 前端在新绑定下恢复类型检查绿 + profiles 页全功能;`current` 单值化;`chain` 面向 `transforms` 重塑;「多选 File Config → 创建 Composition」最小交互;修补 plan 期发现的 spec 缺口(`valid` 白名单写入口,命令面 16→**17**)。本卡收口 = 前端构建绿(§4 铁律 3 出口)。

**Architecture:** 分四层由内向外:①backend 补口(第 17 条命令 `set_profile_valid_fields` 全链:actor 消息→client→facade→ipc→specta);②bindings 再生 + `@nyanpasu/interface` hooks 层重写(`useProfile` 的 7 个 mutation 重映射到新命令;`NormalizedProfile` 崩塌层退役,直接暴露 `ProfileItem` + 判别 helpers);③`mutation-provider` 事件→查询失效映射修补(既有 TODO);④profiles 页组件逐文件适配(27 文件盘点见基线,重映射核心 = `chian-editor-card`(个体 transforms)与 `active-button`(current 单值))+ Composition 创建最小交互。

**Tech Stack:** tauri-specta 生成绑定、TanStack Query(既有失效模式)、`@dnd-kit`(chain 编辑器复用)、zod 表单 schema。

## Global Constraints

- cargo 侧照旧 `$env:CARGO_TARGET_DIR='F:\codex-target\clash-nyanpasu-pr3'`;前端命令在 worktree 根:`pnpm -F interface build`、`pnpm --dir frontend/nyanpasu exec tsc --noEmit`(或仓库既有 typecheck script,以 package.json 为准)。
- 出口判据:`pnpm -F interface build` + 前端 typecheck + `pnpm web:build` 全绿;全仓 `patch_profiles_config`/`.chain` 字段引用零残留(卡片验证行)。
- 每 commit 后端 `cargo build`+`cargo test` 保持绿(Task 0 触后端)。
- i18n:新增文案走 Paraglide 消息文件,构建前 `pnpm --dir frontend/nyanpasu exec paraglide-js compile --project ./project.inlang --outdir ./src/paraglide`(CLAUDE.md §17 已知缺口)。

## 基线事实(2026-07-06 fe-inventory 实测)

- **bindings.ts(2252 行)双代类型共存**:legacy 平铺(`Profiles_Serialize{current: string[], chain: string[], valid, items: Profile_Serialize[]}`,四变体 union)与新域(`ProfileDocument_Serialize{current?: ProfileId|null, global_transforms?: ProfileId[], valid, items: ProfileItem_Serialize[]}`、`ConfigDefinition`(file|composition)、`TransformDefinition`(overlay|script)、`CompositionConfig{base?, extend_proxies_from?, transforms?}`)已并存——T01 已生成,T08 后再生将只余新域 + 新命令。`ProfileId = string` 纯别名(bindings.ts:1196)。
- **`useProfile`(use-profile.ts,196 行)7 mutation 现映射**:query:80 getProfiles;create:104 importProfile /:108 createProfile;update:128 updateProfile;patch:144 patchProfile;sort:157 reorderProfilesByList;upsert:167 patchProfilesConfig(**两种载荷**:active-button 的 `{current:[uid]}` 与 field-filter-card 的 `{valid}`);drop:179 deleteProfile;helper view:70 viewProfile。`NormalizedProfile` = 四变体崩塌(:13-18)。
- **hook 外直调**:`open-locally.tsx:13` 直调 `commands.viewProfile`(同名保留,零改动)。`reorderProfile`(单数)与 `enhanceProfiles` 前端零调用(死绑定)。
- **`current` 消费仅一处**:`active-button.tsx:21`(`current?.find`)与 `:27`(`upsert({current:[uid]})`),同文件 `useActiveProfile` 激活后还调 `useClashConnections().deleteConnections(null)`(无条件断连,legacy UX)。
- **`chain` 消费**:创建表单 schema 默认 null(local/remote-profile-button.tsx);**`chian-editor-card.tsx`(280 行,文件名 typo)= 最大重映射面**——读 `profile.chain`(:179/200/203/213),Apply 经 `patch({...profile, chain})`(:221);仅对 `isProxyProfile` 渲染(`$uid.tsx:53`)。
- **spec 缺口(本卡修)**:`field-filter-card.tsx`(settings/clash)用 `upsert({valid})` 编辑 clash 字段白名单;新 16 条命令与 actor 协议均无 `valid` 写入口。
- **mutation-provider.tsx**:监听 `nyanpasu://mutation`;`'clash_config'`→失效列表**含** profiles key(rebuild 驱动的刷新靠它间接覆盖);`'profiles'`→**不含** profiles key(:43 悬置 TODO)。后端 T07 起 rebuild 发 `clash_config` 事件(UiEventSink 同 URI)——间接链路成立;本卡把 TODO 修掉(前向兼容 `refresh_profiles`)。
- **原始类型直用的 5 组件**(崩塌层外泄):subscription-card/url-editor/option-editor(`RemoteProfile_Serialize`)、local/remote-profile-button(`*Builder_Serialize`)——需换新域等价类型。
- 编辑器窗口:`(editor)/editor/profile/index.tsx` + `hooks.tsx` 的 `useCurrentProfile` 从 `item.type`/`script_type` 推语言——改从 `definition` 判别。
- runtime 命令面:`getRuntimeYaml` 3 个 settings 消费者(命令名不变,零改动);`getPostprocessingOutput` hook 存在但无页面消费;`getRuntimeExists` 全无消费。
- **新域 TS 形状注意**(再生后核):`ProfileItem` 的 metadata 与 definition 均 serde flatten + adjacent tag(YAML wire:`{uid, name, type: 'config'|'transform', config|transform: {...}}`);容器类型名 `ProfileDocument_Serialize`(T01 生成名,若 T08 后再生更名以实物为准,全局别名一处收口)。

## 契约修正(执行后回写 task.md T09 卡 + design §9 表,§5.3)

1. **命令面 16→17**:新增 `set_profile_valid_fields(fields: Vec<String>)`(actor `SetValidFields` 消息 + `ProfilesClient::set_valid_fields` + facade 同名 + ipc + specta)。`valid` 变更影响 whitelist 过滤 → `affects_current` 恒 true(触发 rebuild)。design §9 表补一行;T07/T08 卡 Produces 名单同步。
2. `NormalizedProfile` 崩塌层退役:`useProfile` 直接吐 `ProfileItem`(附 helper fn);判别 helpers(`isConfigItem/isTransformItem/isRemoteItem/scopedTransformsOf`)进 `@nyanpasu/interface` 供页面复用。
3. `upsert` mutation 拆解:`activate`(current 单值)+ `setValidFields`;`patch` 拆 `patchMetadata`/`patchRemoteOptions`/`replaceDefinition` 三 mutation。
4. chain 编辑器语义迁移:编辑对象从「item 平铺 chain」改为「当前 config item 的 scoped `transforms`」(File/Composition 皆有),Apply 走 `replaceProfileDefinition`(定义原子替换);全局链(`global_transforms`)legacy 无 UI,维持无 UI(`setGlobalTransforms` 命令留给将来,与 `enhanceProfiles` 同为前端暂无调用)。
5. 激活后前端 `deleteConnections(null)` 保留(legacy UX);与后端统一中断(用户决策,默认 false 门控)可能双断连——记录为已知重叠,后端选项开启者感知为幂等。
6. mutation-provider 的 `'profiles'` 事件映射补 `RROFILES_QUERY_KEY`(修 :43 TODO;当前后端 rebuild 走 `clash_config` 已间接覆盖,此为前向兼容)。

---

### Task 0: backend 补口——第 17 条命令 `set_profile_valid_fields`

**Files:**

- Modify: `backend/tauri/src/state/profiles/actor.rs`(消息 + handler + 测试)
- Modify: `backend/tauri/src/client/profiles.rs`(client 方法 + 测试)
- Modify: `backend/tauri/src/client/mod.rs`(facade 方法)
- Modify: `backend/tauri/src/ipc.rs` + `backend/tauri/src/specta_export.rs`(命令 + 注册)

**Interfaces:**

- Produces: `ProfilesClient::set_valid_fields(fields: Vec<String>) -> Result<CommitReport, ProfilesError>`;facade `set_profile_valid_fields(fields: Vec<String>) -> Result<()>`(affects_current 恒 true → rebuild);命令 `set_profile_valid_fields`。

- [ ] **Step 1: 写失败测试**(client/profiles.rs tests,仿 set_global_transforms 既有用例):

```rust
    #[tokio::test]
    async fn set_valid_fields_commits_and_always_affects_current() {
        let (client, _guard) = test_client_with_mocks().await; // 复用本文件既有构造 helper 实名
        let report = client
            .set_valid_fields(vec!["dns".into(), "tun".into()])
            .await
            .expect("set valid fields");
        assert!(report.affects_current, "whitelist change must trigger rebuild");
        assert_eq!(report.snapshot.valid, vec!["dns".to_string(), "tun".to_string()]);
    }
```

- [ ] **Step 2: 确认失败**(编译错:方法未定义)。

- [ ] **Step 3: 实现**——actor.rs 消息 enum 增:

```rust
    SetValidFields {
        fields: Vec<String>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
```

handler(仿 `SetGlobalTransforms` 臂,§6.3 事务):

```rust
            ProfilesActorMessage::SetValidFields { fields, reply } => {
                let result = Self::run_write(&myself, state, move |profiles| {
                    profiles.valid = fields;
                    Ok(WriteOutcome {
                        affects: AffectsRule::Always,
                        post_ops: vec![],
                    })
                })
                .await;
                let _ = reply.send(result);
            }
```

(`AffectsRule::Always` 若无此变体,用既有语义等价物——`SetGlobalTransforms` 臂用什么就用什么,whitelist 与全局链同属「恒影响当前」。)client:

```rust
    pub async fn set_valid_fields(&self, fields: Vec<String>) -> Result<CommitReport, ProfilesError> {
        self.call(
            move |reply| ProfilesActorMessage::SetValidFields { fields, reply },
            None,
        )
        .await
    }
```

facade(client/mod.rs,write 统一模式):

```rust
    pub async fn set_profile_valid_fields(&self, fields: Vec<String>) -> Result<()> {
        let report = self.inner.profiles.set_valid_fields(fields).await?;
        self.after_commit(&report).await
    }
```

ipc + specta(仿 set_global_transforms):

```rust
#[tauri::command]
#[specta::specta]
pub async fn set_profile_valid_fields(
    client: State<'_, NyanpasuClient>,
    fields: Vec<String>,
) -> Result {
    client.set_profile_valid_fields(fields).await?;
    Ok(())
}
```

注册表 `// profile` 组尾部加 `ipc::set_profile_valid_fields,`。

- [ ] **Step 4: 验证 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu set_valid_fields`
Expected: PASS;全量绿。

```powershell
git add backend/tauri/src
git commit -m "feat(tauri): add set_profile_valid_fields command (17th, plan-found gap)"
```

---

### Task 1: bindings 再生 + `@nyanpasu/interface` hooks 层重写

**Files:**

- Regenerate: `frontend/interface/src/ipc/bindings.ts`(debug run 触发)
- Modify: `frontend/interface/src/ipc/use-profile.ts`(整文件重写)
- Modify: `frontend/interface/src/ipc/use-profile-content.ts`(save 参数非空化)
- Modify: `frontend/interface/src/ipc/index.ts`(新 helpers 导出;若有 barrel 变化)

- [ ] **Step 1: 再生 bindings**

Run(worktree 根): `cargo run --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`(debug;导出发生在 run() 前段,随后可直接关闭应用/Ctrl+C——BC 已解除,应用应可正常起)
Expected: bindings.ts 更新——旧 13 命令与 `Profiles_Serialize`/`ProfileBuilder_*` 消失,17 条新命令 + `ProfileDocument_Serialize`(实际生成名以此步为准,下述代码统一经类型别名收口)在位。`git diff --stat frontend/interface/src/ipc/bindings.ts` 留痕。

- [ ] **Step 2: 重写 use-profile.ts**(完整新文件骨架,类型名以 Step 1 实物微调):

```typescript
import { unwrapResult } from '@/utils'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  commands,
  type ConfigDefinition_Serialize,
  type NewProfileRequest,
  type ProfileDefinition_Serialize,
  type ProfileDocument_Serialize,
  type ProfileId,
  type ProfileItem_Serialize,
  type ProfileMetadataPatch,
  type RemoteProfileOptionsPatch,
} from './bindings'
import { RROFILES_QUERY_KEY } from './consts'

export type ProfileDocument = ProfileDocument_Serialize
export type ProfileItem = ProfileItem_Serialize

// ---- 判别 helpers(NormalizedProfile 崩塌层的新域替身) ----
export const isConfigItem = (item: ProfileItem) => item.type === 'config'
export const isTransformItem = (item: ProfileItem) => item.type === 'transform'
export const isRemoteItem = (item: ProfileItem) =>
  isConfigItem(item) &&
  'file' in item.config &&
  item.config.file.source.type === 'remote'
/** 当前 item 的 scoped transforms(File/Composition 皆有;Transform item 无) */
export const scopedTransformsOf = (item: ProfileItem): ProfileId[] =>
  isConfigItem(item)
    ? 'file' in item.config
      ? (item.config.file.transforms ?? [])
      : (item.config.composition.transforms ?? [])
    : []

export interface ProfileHelperFn {
  view: () => Promise<unknown>
  update: (option?: RemoteProfileOptionsPatch | null) => Promise<unknown>
  drop: () => Promise<unknown>
}
export type ProfileQueryResultItem = ProfileItem & Partial<ProfileHelperFn>

export type CreateParams =
  | {
      type: 'url'
      data: { url: string; option?: RemoteProfileOptionsPatch | null }
    }
  | {
      type: 'manual'
      data: { request: NewProfileRequest; fileData: string | null }
    }

export const useProfile = (options?: { without_helper_fn?: boolean }) => {
  const queryClient = useQueryClient()
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] })

  const query = useQuery({
    queryKey: [RROFILES_QUERY_KEY],
    queryFn: async () => {
      const result = unwrapResult(await commands.getProfiles())
      if (options?.without_helper_fn) return result
      return {
        ...result,
        items: (result.items ?? []).map((item) => ({
          ...item,
          view: () => commands.viewProfile(item.uid),
          update: (option?: RemoteProfileOptionsPatch | null) =>
            update.mutateAsync({ uid: item.uid, option: option ?? null }),
          drop: () => drop.mutateAsync(item.uid),
        })),
      }
    },
  })

  const create = useMutation({
    mutationFn: async (params: CreateParams) => {
      if (params.type === 'url') {
        return unwrapResult(
          await commands.importProfile(
            params.data.url,
            params.data.option ?? null,
          ),
        )
      }
      return unwrapResult(
        await commands.createProfile(params.data.request, params.data.fileData),
      )
    },
    onSuccess: invalidate,
  })

  const update = useMutation({
    // 订阅刷新(旧 update 语义;非 Remote 后端返回域错误)
    mutationFn: async ({
      uid,
      option,
    }: {
      uid: ProfileId
      option?: RemoteProfileOptionsPatch | null
    }) => unwrapResult(await commands.updateProfile(uid, option ?? null)),
    onSuccess: invalidate,
  })

  const patchMetadata = useMutation({
    mutationFn: async ({
      uid,
      patch,
    }: {
      uid: ProfileId
      patch: ProfileMetadataPatch
    }) => unwrapResult(await commands.patchProfileMetadata(uid, patch)),
    onSuccess: invalidate,
  })

  const patchRemoteOptions = useMutation({
    mutationFn: async ({
      uid,
      patch,
    }: {
      uid: ProfileId
      patch: RemoteProfileOptionsPatch
    }) => unwrapResult(await commands.patchRemoteProfileOptions(uid, patch)),
    onSuccess: invalidate,
  })

  const replaceDefinition = useMutation({
    mutationFn: async ({
      uid,
      definition,
    }: {
      uid: ProfileId
      definition: ProfileDefinition_Serialize
    }) =>
      unwrapResult(await commands.replaceProfileDefinition(uid, definition)),
    onSuccess: invalidate,
  })

  const activate = useMutation({
    mutationFn: async (uid: ProfileId | null) =>
      unwrapResult(await commands.activateProfile(uid)),
    onSuccess: invalidate,
  })

  const setValidFields = useMutation({
    mutationFn: async (fields: string[]) =>
      unwrapResult(await commands.setProfileValidFields(fields)),
    onSuccess: invalidate,
  })

  const sort = useMutation({
    mutationFn: async (uids: ProfileId[]) =>
      unwrapResult(await commands.reorderProfilesByList(uids)),
    onSuccess: invalidate,
  })

  const drop = useMutation({
    mutationFn: async (uid: ProfileId) =>
      unwrapResult(await commands.deleteProfile(uid)),
    onSuccess: invalidate,
  })

  return {
    query,
    create,
    update,
    patchMetadata,
    patchRemoteOptions,
    replaceDefinition,
    activate,
    setValidFields,
    sort,
    drop,
  }
}
```

(`ProfileDefinition_Serialize` 若生成名不同(如 item 内联 adjacent tag 无独立类型),以 `Extract<ProfileItem, {type:'config'}>['config']` 型式派生别名收口,页面代码不感知。)

- [ ] **Step 3: use-profile-content.ts**——`upsert` 的 `fileData: string`(非空);其余(query key、readProfileFile)零改动。

- [ ] **Step 4: interface 构建**

Run: `pnpm -F interface build`
Expected: 绿(nyanpasu 包此时仍红,属预期——Task 3 收口)。

```powershell
git add frontend/interface
git commit -m "feat(interface)!: rewrite profile hooks over domain commands"
```

---

### Task 2: mutation-provider 事件映射修补

**Files:**

- Modify: `frontend/interface/src/provider/mutation-provider.tsx`(:20/:43 两处 TODO)

- [ ] **Step 1: 实现**——`PROFILES_MUTATION_KEYS` 加入 `RROFILES_QUERY_KEY`(import 自 consts);移除对应 `// TODO: profiles hook refetch` 注释。`'nyanpasu_config'` 分支的同名 TODO 保留(非本域)。

- [ ] **Step 2: Commit**

```powershell
git add frontend/interface/src/provider/mutation-provider.tsx
git commit -m "fix(interface): refetch profiles query on backend profiles mutation event"
```

---

### Task 3: profiles 页组件逐文件适配

**Files(全量清单,按重映射强度排序):**

- Modify: `frontend/nyanpasu/src/pages/(main)/main/profiles/$type/detail/_modules/chian-editor-card.tsx`(数据模型重映射:`profile.chain` → 当前 config item 的 scoped `transforms`;Apply → `replaceDefinition`)
- Modify: `.../detail/_modules/active-button.tsx`(current 单值 + activate)
- Modify: `.../$type/_modules/utils.ts`(分类器按 definition 判别重写)
- Modify: `.../$type/_modules/{local,remote}-profile-button.tsx` + `chain-profile-import.tsx`(表单产出 `NewProfileRequest`;schema 中 `chain` 字段删除)
- Modify: `.../detail/_modules/{profile-name-editor,subscription-card,subscription-url-editor,update-option-editor}.tsx`(patch 拆三:name→`patchMetadata`;URL 变更→`replaceDefinition`(定义含 url);fetch 选项→`patchRemoteOptions`;刷新进度→`update`)
- Modify: `.../detail/_modules/delete-profile.tsx`、`profiles-list.tsx`、`profiles-navigate.tsx`、`profile-quick-import.tsx`、`$type/detail/$uid.tsx`、`(editor)/editor/_modules/hooks.tsx`(字段访问随 `ProfileItem` 形状与 hook 返回名微调)
- Modify: `frontend/nyanpasu/src/pages/(main)/main/settings/clash/_modules/field-filter-card.tsx`(`upsert({valid})` → `setValidFields(fields)`)
- 不动:`open-locally.tsx`(viewProfile 同名)、`view-content.tsx`、`getRuntimeYaml` 三消费者。

- [ ] **Step 1: utils.ts 分类器**(完整替换,页面判别的单一事实源):

```typescript
import {
  isConfigItem,
  isTransformItem,
  type ProfileItem,
} from '@nyanpasu/interface'

/** 可激活 = Config 定义(File/Composition) */
export const isProxyProfile = isConfigItem
/** 链编辑候选 = Transform 定义(Overlay/Script) */
export const isChainProfile = isTransformItem

export const categoryProfiles = (items?: ProfileItem[] | null) => {
  const list = items ?? []
  return {
    configs: list.filter(isConfigItem),
    transforms: list.filter(isTransformItem),
  }
}
```

(旧枚举 `ProfileType.JavaScript/Lua/Merge` 类消费点改为按 `item.transform` 内 tag(overlay/script)与 `script.runtime` 判别;`consts.ts` 的映射表同步瘦身。)

- [ ] **Step 2: active-button.tsx**——`useActiveProfile` 核心两行:

```typescript
const isActive = data?.current === profile.uid
// ...
await activate.mutateAsync(profile.uid)
```

(`upsert` 引用全换 `activate`;激活后的 `deleteConnections.mutateAsync(null)` **保留**——契约修正 5。)

- [ ] **Step 3: chian-editor-card.tsx 重映射**(核心逻辑;DnD 骨架复用):

数据源与提交换轴——

```typescript
// 读:当前 config item 的 scoped transforms(替代 profile.chain)
const activeUids = useMemo(() => scopedTransformsOf(profile), [profile])
// 候选列:全部 Transform item(替代 JS/Lua/Merge 过滤)
const candidates = useMemo(
  () => categoryProfiles(profiles?.items).transforms,
  [profiles?.items],
)
// Apply:重建 definition,transforms 换为编辑结果,原子替换
const handleApply = async () => {
  if (!isConfigItem(profile)) return
  const definition =
    'file' in profile.config
      ? {
          type: 'config' as const,
          config: {
            ...profile.config,
            file: {
              ...profile.config.file,
              transforms: chainsUids[ColumnType.Active],
            },
          },
        }
      : {
          type: 'config' as const,
          config: {
            ...profile.config,
            composition: {
              ...profile.config.composition,
              transforms: chainsUids[ColumnType.Active],
            },
          },
        }
  await replaceDefinition.mutateAsync({ uid: profile.uid, definition })
}
```

(内联对象展开的具体嵌套形状以再生后的 `ProfileDefinition_Serialize` adjacent-tag 实物为准——原则:除 `transforms` 外逐字段保留原定义。组件 props 从 `NormalizedProfile` 改 `ProfileItem`;两列 state/DnD/样式零改动。)

- [ ] **Step 4: 创建表单三件**——共同产出 `NewProfileRequest`:

```typescript
// remote-profile-button.tsx 提交体(url 导入走 create({type:'url'}) 不变;
// 此处为「手动 Remote」路径——如旧表单仅 url 导入则该按钮整体走 'url' 分支,无 manual Remote)
// local-profile-button.tsx 提交体:
const request: NewProfileRequest = {
  metadata: { name: values.name, desc: values.desc ?? null },
  definition: {
    type: 'config',
    config: {
      type: 'file',
      file: {
        source: { type: 'local', binding: { type: 'managed', file: 'pending.yaml' } },
        transforms: [],
      },
    },
  },
}
await create.mutateAsync({ type: 'manual', data: { request, fileData: values.fileData ?? '' } })
// chain-profile-import.tsx(Merge→Overlay / Script):
definition: {
  type: 'transform',
  transform: values.kind === 'overlay'
    ? { type: 'overlay', overlay: { source: managedPendingSource } }
    : { type: 'script', script: { source: managedPendingSource, runtime: values.runtime } },
}
```

(`managed` 占位路径由后端 Add 重写为 `{uid}.{ext}`(T04 语义),前端只需合法占位;tag 嵌套形状同上以 bindings 实物为准。zod schema 删除 `chain` 字段与默认值。)

- [ ] **Step 5: detail 编辑四件**——`profile-name-editor` → `patchMetadata({uid, patch:{name}})`;`update-option-editor` → `patchRemoteOptions({uid, patch})`(双 Option 三态:未动字段不出现在 patch);`subscription-url-editor`(URL 属定义)→ 组装含新 url 的 Remote 定义走 `replaceDefinition`;`subscription-card` 刷新按钮 → `update`(refresh)。`RemoteProfile_Serialize` import 换 `ProfileItem` + 判别。

- [ ] **Step 6: field-filter-card.tsx** → `setValidFields.mutateAsync(fields)`。

- [ ] **Step 7: 编辑器 hooks**——`useCurrentProfile` 的语言推导:

```typescript
const language = isTransformItem(item)
  ? 'script' in item.transform
    ? item.transform.script.runtime === 'lua'
      ? 'lua'
      : 'javascript'
    : 'yaml'
  : 'yaml'
const readOnly = isRemoteItem(item)
```

- [ ] **Step 8: 类型检查驱动收尾**

Run: `pnpm --dir frontend/nyanpasu exec tsc --noEmit`(以 package.json 实名 script 为准)
按报错清单逐文件收残(profiles-list/navigate/quick-import/$uid 的字段访问微调)。
Expected: 0 error。

- [ ] **Step 9: Commit**

```powershell
git add frontend
git commit -m "feat(nyanpasu)!: adapt profiles pages to domain model (single current, transforms)"
```

---

### Task 4: 「多选 File Config → 创建 Composition」最小交互(design §11.3)

**Files:**

- Create: `frontend/nyanpasu/src/pages/(main)/main/profiles/$type/_modules/create-composition-button.tsx`
- Modify: `profiles-list.tsx`(多选态 + 入口按钮挂载)

- [ ] **Step 1: 实现**(最小交互:列表长按/复选进入多选 → 选 ≥2 个 File Config → 按钮弹名称输入 → 提交):

```typescript
export const CreateCompositionButton = ({
  selected,
  onDone,
}: {
  selected: ProfileId[]
  onDone: () => void
}) => {
  const { create } = useProfile()
  const [name, setName] = useState('Combined Profile')
  const submit = async () => {
    const request: NewProfileRequest = {
      metadata: { name, desc: null },
      definition: {
        type: 'config',
        config: {
          type: 'composition',
          composition: {
            base: null,
            extend_proxies_from: selected,
            transforms: [],
          },
        },
      },
    }
    await create.mutateAsync({
      type: 'manual',
      data: { request, fileData: null },
    })
    onDone()
  }
  // ...按钮 + 名称对话框 JSX,复用仓库既有 Dialog/BaseDialog 组件
}
```

多选态:`profiles-list.tsx` 增 `selectMode: boolean` + `selectedUids: Set<ProfileId>` 本地 state,卡片复选框仅对 `isConfigItem && 'file' in item.config` 项显示;完整管理界面(编辑成员/base 切换)为非目标,不做。

- [ ] **Step 2: 冒烟**——选两个 File Config → 创建 → 列表出现 Composition 项、可激活、无物化文件(view 报 `ProfileHasNoFile` toast 文案友好)。

- [ ] **Step 3: Commit**

```powershell
git add frontend/nyanpasu
git commit -m "feat(nyanpasu): minimal multi-select composition creation"
```

---

### Task 5: 出口判据 + 契约回写

- [ ] **Step 1: 全套构建**

Run: `pnpm -F interface build && pnpm web:build` + 后端 `cargo build`/`cargo test`
Expected: 全绿。

- [ ] **Step 2: 残留 grep**

```powershell
Select-String -Path frontend -Pattern "patch_profiles_config|patchProfilesConfig|Profiles_Serialize|ProfileBuilder" -Recurse -Exclude *.map
Select-String -Path frontend/interface/src,frontend/nyanpasu/src -Pattern "\.chain\b|chain:" -Recurse
```

Expected: 双双零命中(bindings.ts 再生后不含旧类型;`chain` 仅许可命中为无关词根,逐条目检)。

- [ ] **Step 3: 回写 task.md**(T09 卡执行修正块:17 条命令勘误 + `NormalizedProfile` 退役 + chain→scoped transforms 语义 + deleteConnections 重叠注记 + mutation-provider 修补 + `ProfileDocument` 实际类型名)+ design §9 表补 `set_profile_valid_fields` 行。

```powershell
git add docs
git commit -m "docs(pr3): record T09 execution addenda (17th command, chain remap)"
```

---

## Self-Review 结论

- 覆盖:卡片 5 项 Files 全落位 + fe-inventory 全部 27 文件逐一归类(改/不动);spec 缺口(valid 写入口)以 Task 0 补;卡片验证三行(类型检查绿/冒烟/残留 grep)在 Task 3.8/4.2/5 对应。
- 无占位符:所有「以实物为准」均为 bindings 再生后的 tag 嵌套形状(Step 1 一次性显形,别名收口单点适配),核心逻辑代码完整。
- 类型一致性:hook 返回名(activate/patchMetadata/patchRemoteOptions/replaceDefinition/setValidFields/update/sort/drop/create)与 Task 3 各组件消费一一对应;`NewProfileRequest` 构造三处(local/chain-import/composition)同构。
