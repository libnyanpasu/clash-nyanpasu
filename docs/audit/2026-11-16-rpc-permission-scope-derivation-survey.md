# RPC 权限 scope 推导横向调研报告

> 调研日期：2026-11-16。问题：RPC 命令的权限 scope 能否不逐条手写，而是由路由、规范化的操作名或注解生成推导出来，手工标注仅作兜底。本报告只讨论权限模型本身，不以本仓库现有命令结构为前提。来源以官方文档与源码为主，置信度见各节。

## 结论

主流框架给接口定权限的方式可分三类：**从路由推导**（Kubernetes RBAC、Casbin、Spring URL 规则、Quarkus、ASP.NET 路由组）、**从规范化的操作名推导**（AWS IAM、Google Cloud IAM、gRPC、ABP、Shiro）、**由注解或生成器产出**（Spring 方法安全、Jakarta、ASP.NET `[Authorize]`、Tauri v2 ACL、actix-web-grants）。三类可以组合，成熟的系统通常是“推导为主、注解覆盖、中心策略兜底”。

能稳定工作的推导系统有四个共同前提：

1. **标识符是稳定契约**：URL 或操作名改动即破坏性变更，所以“改名改权限”不会悄悄发生。
2. **中心策略、默认拒绝**：标识符映射到权限的规则集中在一处，未命中即拒绝。
3. **秘密是独立资源，不靠动词区分**：Kubernetes 的 `secrets`、GCP 的 `versions.access`、AWS 的 List/Read 都把“读秘密”建模为单独的资源或权限。没有任何一个系统用动词推导“能否读秘密”。
4. **有可审查的清单**：生成的权限清单或快照随代码一起评审，推导结果的变化在 diff 中可见。

据此推荐的模型：规范化的层级操作名 `namespace.resource[.subresource].verb`；命名空间决定写操作的领域 scope，封闭动词表决定读写类别，声明为敏感的子资源决定 `sensitive_read`；注解只用于覆盖，且必须带理由；构建期生成权限清单并做快照测试；未知命名空间、动词、子资源一律编译失败。见[推荐模型](#推荐模型)。

置信度：前提 1–3 高（多个独立官方来源一致）；推荐模型整体中高（由先例类比得出，未在实现上验证）。

## 三类做法

### 从路由推导

| 框架                   | 推导来源                                                                                                                                    | 默认 / 覆盖与优先级                                               | 未覆盖时                                   | 读秘密                                                                                                |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | ------------------------------------------ | ----------------------------------------------------------------------------------------------------- |
| Kubernetes RBAC        | apiserver 的 `RequestInfo` 从 HTTP 方法与“是否带名字”推出 verb（get/list、delete/deletecollection），从 URL 推出 group/resource/subresource | 角色只做加法，无 deny 规则                                        | 默认拒绝                                   | 独立资源 `secrets`、子资源 `pods/log`；内置 `view` 角色不含 Secret；对 secrets 的 `list` 即可读到内容 |
| Casbin（含 casbin-rs） | `keyMatch(path)` 加 `regexMatch(method)`；axum-casbin 直接取请求的 path 与 method                                                           | 任一策略允许即允许（由 effect 表达式决定）                        | 默认拒绝                                   | 无专门机制                                                                                            |
| Spring Security URL    | request matcher 匹配路径与方法                                                                                                              | 只取第一条命中的规则；官方建议末尾加 `denyAll` 把规则集变成白名单 | 须显式加兜底规则                           | 本层无                                                                                                |
| Quarkus                | 配置中的路径与方法权限集                                                                                                                    | 最长路径优先；写明 HTTP 方法的规则优先                            | 须开启 `deny-unannotated-endpoints`        | 无                                                                                                    |
| ASP.NET Core 路由组    | `MapGroup().RequireAuthorization()`、Razor Pages `AuthorizeFolder`、启动期 `IApplicationModelConvention`                                    | 多个策略同时生效时须全部通过；`[AllowAnonymous]` 优先             | `FallbackPolicy` 默认为 `null`，须显式设置 | 资源级 handler，无字段过滤                                                                            |
| axum-login             | `permission_required!` 以 `route_layer` 挂在路由组上                                                                                        | 组级                                                              | 仅组内生效                                 | 无                                                                                                    |

### 从规范化的操作名推导

| 框架             | 推导来源                                                                                                                     | 默认 / 覆盖与优先级                           | 未覆盖时              | 读秘密                                                                                                        |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- | --------------------- | ------------------------------------------------------------------------------------------------------------- |
| AWS IAM          | 动作名即 API 操作名（`s3:GetObject`）；每个动作另标访问级别 List / Read / Write / Permissions management / Tagging           | 显式 deny 压过 allow                          | 隐式拒绝              | List 与 Read 分级（List “看不到内容”）；托管策略 `ViewOnlyAccess` 有 `ListBucket` 无 `GetObject`              |
| Google Cloud IAM | 权限名 `service.resource.verb`；方法遵循 AIP 标准方法（Get/List/Create/Update/Delete）                                       | Viewer ⊂ Editor ⊂ Owner，另有预定义角色       | 默认拒绝              | 载荷单列权限：`secretmanager.versions.access`（载荷）与 `secrets.get`（元数据）分开，Viewer/Editor 均不含前者 |
| gRPC             | 完整方法名 `/package.Service/Method`；authz 策略按 `paths` 匹配                                                              | `deny_rules` 先于 `allow_rules`               | 默认拒绝              | 无                                                                                                            |
| ABP              | 层级权限名（`BookStore.Books.Create`）集中定义；自动控制器由方法名前缀推 HTTP 动词；`CrudAppService` 按操作设策略名          | 子权限依赖父权限                              | 普通方法不受保护      | 无                                                                                                            |
| Apache Shiro     | 通配权限 `domain:action:instance`；`HttpMethodPermissionFilter` 把 GET/HEAD 映射为 `read`、POST 为 `create`、PUT 为 `update` | 通配隐含更窄权限（`user:*` 隐含 `user:view`） | 取决于 URL 过滤链配置 | 依靠 instance 段区分                                                                                          |

### 由注解或生成器产出

| 框架              | 机制                                                                                                                                        | 默认 / 覆盖与优先级                | 未覆盖时                                              | 读秘密                                                             |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------- | ----------------------------------------------------- | ------------------------------------------------------------------ |
| Tauri v2 ACL      | 构建期为每条命令**生成** `allow-<cmd>` / `deny-<cmd>`；权限可组合成权限集（含 `default` 集）；capability 把权限授予窗口、webview 或远程 URL | 作用域上 deny 优先（命令级未核实） | 打包内窗口默认放行应用命令；远程来源须显式 capability | 权限可带 allow/deny 作用域                                         |
| Spring 方法安全   | `@PreAuthorize` / `@Secured` 标在类或方法上，可用元注解模板                                                                                 | 方法级覆盖类级                     | 未注解的方法不受保护                                  | `@PostFilter`；`@AuthorizeReturnObject` 在运行时保护返回对象的字段 |
| Jakarta           | `@RolesAllowed`                                                                                                                             | 方法值覆盖类值                     | 默认放行                                              | 无                                                                 |
| actix-web-grants  | `#[protect(...)]` 标在 handler 上                                                                                                           | 守卫拒绝返回 404                   | 默认放行                                              | 无                                                                 |
| Cedar（策略引擎） | schema 中声明 action 与 action group                                                                                                        | `forbid` 压过 `permit`             | 无匹配 permit 即拒绝                                  | 实体属性                                                           |

另：tonic 的 `Interceptor` 只拿到 `Request<()>`，官方建议复杂逻辑用 tower 层；biscuit-auth 用可逐层收窄的 Datalog 块；oso 开源库已归档、不再维护。

## 横向发现

1. **推导依赖稳定标识符**（置信度高）。Kubernetes 与 AWS 能放心从 URL、API 名推导，是因为它们本身就是对外冻结的契约。标识符随意改名的系统，用名字推导等于让重命名静默改变权限。
2. **注解系统默认放行，推导系统默认拒绝**（置信度高）。Spring、Jakarta、ASP.NET 的拒绝兜底都要显式开启（`denyAll`、`FallbackPolicy`、`deny-unannotated`）；Kubernetes、AWS、GCP、gRPC、Cedar 天然默认拒绝。
3. **秘密是资源，不是动词**（置信度高）。三大云和 Kubernetes 一致把“读秘密”做成独立资源、子资源或独立权限；只读角色明确排除它。唯一按类型或字段处理的是 Spring `@AuthorizeReturnObject`，且是运行时过滤。
4. **覆盖有明确优先级**（置信度高）。常见顺序：显式 deny ＞ 更具体的规则（方法级、最长路径）＞ 分组默认 ＞ 全局兜底。
5. **生成清单是防漂移的主要手段**（置信度中）。Tauri 生成每命令权限文件，AWS 公布每个动作的访问级别表；权限变化以清单 diff 的形式进入评审。
6. **动词有歧义时用自定义动词**（置信度中）。Kubernetes 的 `pods/exec` 走 `create`、AIP 允许自定义方法，说明“有副作用的读”“触发型操作”需要显式归类，不能硬套 get/update。

## 推荐模型

### 标识符

每个操作有唯一的规范名：

```text
<namespace>.<resource>[.<subresource>].<verb>

proxy.groups.list
proxy.selection.update
proxy.delay.test          # 自定义动词
profile.items.create
profile.content.get       # 敏感子资源
clash.config.secret.get   # 敏感子资源
system.tun.update
remote.tokens.create
```

规范名即线上方法名，视为冻结契约：改名按破坏性变更处理，并在清单快照中显示为“删除 + 新增”。

### 推导规则（中心表，一处定义）

| 输入                  | 规则                                                                                                                | 先例                                             |
| --------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| 动词                  | 封闭枚举。读类：`get`、`list`、`watch`；写类：`create`、`update`、`patch`、`delete`；自定义动词必须在表中登记其类别 | Kubernetes verb、AIP 标准方法                    |
| 命名空间 × 写类动词   | 命名空间表给出领域 scope：`proxy`、`profile`、`settings`、`system`、`remote_admin`                                  | Kubernetes apiGroup、ASP.NET 路由组、GCP service |
| 读类动词              | 默认 `read`                                                                                                         | AWS Read、Kubernetes `view`                      |
| 敏感子资源 × 读类动词 | 子资源表中标记为敏感的（如 `content`、`secret`、`raw`、`credentials`）提升为 `sensitive_read`                       | Kubernetes `secrets`、GCP `versions.access`      |
| 自定义动词            | 按动词表登记的类别处理；有副作用的读（如测速）登记为写类并归入其命名空间                                            | Kubernetes `pods/exec`                           |

### 覆盖与优先级

1. 显式注解（如 `#[rpc(scope = system, reason = "...")]`）只用于推导不出或推导不当的情形。
2. 注解**提升**权限可直接写；注解**降低**到推导结果之下必须带 `reason`，清单中标记来源为 `override`。
3. 优先级：显式 deny（若引入）＞ 注解 ＞ 敏感子资源规则 ＞ 命名空间 × 动词推导。

### 默认拒绝

- 未登记的命名空间、动词、子资源：编译失败。
- 一个操作在某传输上未暴露：拒绝（暴露面与 scope 仍是两个独立维度，暴露面按 Tauri capability 的思路按调用来源授予）。
- 主体未持有所需 scope：拒绝，错误不泄露内部细节。

### 生成与防漂移

- 由宏或构建脚本生成权限清单，每行包含：规范名、暴露的传输、所需 scope、**推导来源**（`derived` / `sensitive-subresource` / `override`）。
- 清单作为快照测试的固定输入；scope 或来源的任何变化都出现在评审 diff 中。
- 能力发现接口（如 `describe`）直接返回清单内容，客户端（CLI、MCP）据此过滤，不另维护一份表。

### 第二道防线（可选）

在源头把秘密字段建成 newtype（如 `ClashSecret`、`SubscriptionUrl`），由 derive 计算响应类型的敏感级；若某个推导为 `read` 的操作返回了敏感类型，构建或测试失败。它对应 Spring `@AuthorizeReturnObject` 的思路，但放在编译期，用来拦住“往非敏感资源里加了秘密字段”这一类命名规则管不到的泄露。

## 风险与对策

| 风险                                   | 对策                                                         |
| -------------------------------------- | ------------------------------------------------------------ |
| 改名静默改变权限                       | 规范名视为冻结契约；清单快照把改名显示为删除加新增           |
| 秘密混入非敏感资源                     | 敏感子资源规则；可选的类型标记作为第二道防线                 |
| 动词归类不当（有副作用的读、触发操作） | 封闭动词表，自定义动词必须登记类别                           |
| 通配或分组规则过宽                     | 不提供通配授权；分组只决定默认值，单个操作的结果都落在清单里 |
| 注解被用来随意降权                     | 降权必须带理由，清单标记来源，评审可见                       |

## 待决问题

1. 命名空间粒度：按领域（`proxy`、`profile`）还是按子系统（`clash`、`core`）划分；二者混用会让同一资源出现两个前缀。
2. 每个操作只要求一个 scope，还是允许多个 scope 同时满足（ASP.NET 的多策略同时通过）。
3. 是否引入显式 deny 规则；Kubernetes 只做加法，AWS、gRPC、Cedar 有 deny。
4. 自定义动词清单与各自的类别。
5. 敏感子资源的初始列表。

## 局限

- 各框架的行为以官方文档和源码为准，未逐一编写验证程序。
- 以下几点只有单一来源或转述来源：Tauri 命令级是否 deny 优先、Shiro 动词映射表的原文、ABP 非 GET 前缀的完整规则。
- 推荐模型由先例类比得出，没有在实现上验证；规范名改造的迁移成本不在本报告范围内。

## 来源

- Kubernetes：[Authorization](https://kubernetes.io/docs/reference/access-authn-authz/authorization/)；[RBAC](https://kubernetes.io/docs/reference/access-authn-authz/rbac/)；[RBAC good practices](https://kubernetes.io/docs/concepts/security/rbac-good-practices/)；[`requestinfo.go`](https://raw.githubusercontent.com/kubernetes/kubernetes/master/staging/src/k8s.io/apiserver/pkg/endpoints/request/requestinfo.go)
- AWS：[Access level summaries](https://docs.aws.amazon.com/IAM/latest/UserGuide/access_policies_understand-policy-summary-access-level-summaries.html)；[Policy evaluation: deny vs allow](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_policies_evaluation-logic_policy-eval-denyallow.html)；[ViewOnlyAccess](https://docs.aws.amazon.com/aws-managed-policy/latest/reference/ViewOnlyAccess.html)
- Google Cloud：[Roles overview](https://docs.cloud.google.com/iam/docs/roles-overview)；[Secret Manager access control](https://docs.cloud.google.com/secret-manager/docs/access-control)
- gRPC：[`interceptor.go`](https://raw.githubusercontent.com/grpc/grpc-go/master/interceptor.go)；[authz policy example](https://raw.githubusercontent.com/grpc/grpc-go/master/examples/data/rbac/policy.json)
- Casbin：[Supported models](https://casbin.apache.org/docs/supported-models/)；[keyMatch model](https://raw.githubusercontent.com/casbin/casbin/master/examples/keymatch_model.conf)；[axum-casbin](https://github.com/casbin-rs/axum-casbin)
- Spring Security：[Authorize HTTP requests](https://docs.spring.io/spring-security/reference/servlet/authorization/authorize-http-requests.html)；[Method security](https://docs.spring.io/spring-security/reference/servlet/authorization/method-security.html)
- Jakarta：[Annotations 2.1](https://jakarta.ee/specifications/annotations/2.1/annotations-spec-2.1.html)
- Apache Shiro：[Permissions](https://shiro.apache.org/permissions.html)；[`HttpMethodPermissionFilter.java`](https://raw.githubusercontent.com/apache/shiro/main/web/src/main/java/org/apache/shiro/web/filter/authz/HttpMethodPermissionFilter.java)
- Quarkus：[Authorization of web endpoints](https://quarkus.io/guides/security-authorize-web-endpoints-reference)
- ASP.NET Core：[Policies](https://learn.microsoft.com/en-us/aspnet/core/security/authorization/policies)；[Route handlers](https://learn.microsoft.com/en-us/aspnet/core/fundamentals/minimal-apis/route-handlers)；[Razor Pages conventions](https://learn.microsoft.com/en-us/aspnet/core/razor-pages/security/authorization/conventions)；[Application model](https://learn.microsoft.com/en-us/aspnet/core/mvc/controllers/application-model)
- ABP：[Authorization](https://abp.io/docs/latest/framework/fundamentals/authorization)；[Auto API controllers](https://abp.io/docs/latest/framework/api-development/auto-controllers)；[Application services](https://abp.io/docs/latest/framework/architecture/domain-driven-design/application-services)
- Tauri v2：[Plugin development](https://v2.tauri.app/develop/plugins/)；[Permissions](https://v2.tauri.app/security/permissions/)；[Capabilities](https://v2.tauri.app/security/capabilities/)；[Scopes](https://v2.tauri.app/security/scope/)；[`AppManifest`](https://docs.rs/tauri-build/latest/tauri_build/struct.AppManifest.html)
- Cedar：[Authorization](https://docs.cedarpolicy.com/auth/authorization.html)；[Schema](https://docs.cedarpolicy.com/schema/human-readable-schema.html)
- Rust 生态：[protect-endpoints（actix-web-grants）](https://github.com/DDtKey/protect-endpoints)；[axum-login](https://docs.rs/axum-login/latest/axum_login/)；[tonic `Interceptor`](https://docs.rs/tonic/latest/tonic/service/trait.Interceptor.html)；[biscuit-auth](https://docs.rs/biscuit-auth/latest/biscuit_auth/)；[oso](https://github.com/osohq/oso)
