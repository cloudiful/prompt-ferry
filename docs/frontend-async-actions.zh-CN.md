# 前端异步操作

[English](frontend-async-actions.md)

远程写操作在共享守卫中申请 key。`src/lib/async-action.ts` 持有 pending 映射并导出
`runAsyncAction`、`getPendingActions` 和 `subscribeToAsyncActions`；
`src/lib/use-async-action.ts` 是 React 适配层（`useAsyncAction`），页面与领域模块都通过它
使用守卫。每个互斥资源只有一份守卫实例，该资源的所有入口使用相同 key。key 已被占用时返回
`{ executed: false }`；UI 处理器只有在 `executed` 为 true 时才执行成功副作用，包括通知、导航、
关闭对话框和重置草稿。

key 由领域构建，而不是由组件构建。`src/stores/record-action-types.ts` 拥有这套语法：
`recordActionKey(prefix, id)` 生成 `<prefix>:<id>`，尚未持久化的记录共用一个
`<prefix>:create` 槽位，`capturePayload` 在第一次 await 之前快照请求体。各领域再补充自己的
辅助函数（`endpointActionKey`、`modelRouteActionKey`、`mcpActionKey`、`userActionKey`、
`clientKeyActionKey`、`relayActionKey`、`approvalActionKey`、`billingPriceRuleActionKey`、
`settingKey`、`usageConversationActionKey`）。

守卫应覆盖完整写操作及必要刷新。等待前捕获记录身份、所选用户和提交载荷。将 pending 状态传给
冲突控件：禁用冲突输入和关闭操作，保持无关资源可用，仅让实际发起操作的控件显示 spinner。错误
保持可见，草稿可重试。共享 store action 外不创建组件级守卫。

## 操作覆盖

- **Endpoint 与模型路由** — `endpoint:<id>` 覆盖 endpoint 保存/删除/切换，以及 OAuth 设备/浏览器登录启动、每次轮询、完成、刷新和清除。`modelroute:<id>` 覆盖路由保存/删除/切换。探测使用独立的 `networktest:<domain>:<id>`。Pending 状态传递到行和 endpoint 对话框。OAuth 控件通过对话框接收页面所有的回调；每次设备轮询独立申请 key，人工授权期间不占用 key。证据：`src/react-tests/endpoints.test.tsx`、`src/react-tests/endpoints-oauth.test.tsx`、`src/react-tests/model-routes.test.tsx`、`src/react-tests/endpoints-model.test.ts`。
- **MCP 服务** — `mcp:<id>` 覆盖保存/删除，`mcp:create` 覆盖创建；目录探测使用 `mcptest:<id>`。Pending 状态传递到服务行和对话框。目录编辑是本地草稿。证据：`src/react-tests/mcp.test.tsx`。
- **用户** — `user:<id>` 覆盖保存/删除/密码重置；`user:create` 覆盖创建。Pending 状态传递到用户行/对话框及重置控件。证据：`src/react-tests/users.test.tsx`。
- **客户端 key** — `clientkey:<id>` 覆盖标签/保存/删除/切换；创建使用已捕获所选用户对应的 key。Pending 状态传递到 key 行/对话框。证据：`src/react-tests/api-keys.test.tsx`。
- **Relay** — `relay:<id>` 覆盖保存/删除/重连。Owner pending 状态传递到行操作和对话框；写入及刷新期间禁用字段和关闭操作，仅发起操作显示 spinner。证据：`src/react-tests/relays.test.tsx`。
- **Relay Host** — `host:local` 将角色/设置/重启视作同一 host 资源。共享 pending 状态会禁用互斥控件，仅发起操作显示 spinner；角色/设置完成时保留提交后编辑的字段。证据：`src/react-tests/relay-host.test.tsx`、`src/react-tests/relay-host-model.test.ts`。
- **设置** — `setting:<section>` 覆盖各设置区：`rawRetention`、`streamDeltaBatching`、`modelRouteWhitelist`、`relayIpWhitelist`、`llmReview`、`storage` 和 `cacheAlert`。请求内容记录与使用保留期是同一个存储值，因此共用 `setting:rawRetention`；被挡下的表单保持脏状态，在 key 释放后重试。证据：`src/react-tests/settings-requests.test.tsx`、`src/react-tests/settings-review.test.tsx`、`src/react-tests/settings-network.test.tsx`、`src/react-tests/settings-storage.test.tsx`、`src/react-tests/settings-cache-alert.test.tsx`。
- **脱敏** — `redaction:rules` 覆盖保存，`redaction:preview` 覆盖非持久化预览。两者反馈独立；过期预览响应会被忽略。证据：`src/react-tests/redaction-model.test.ts`、`src/react-tests/redaction.test.tsx`。
- **配置归档** — `configArchive:export` 与 `configArchive:import` 是互相独立的 key，导入 key 由预览和确认应用共用。向导保留确认、文件校验和可重试草稿状态。证据：`src/react-tests/config-archive.test.tsx`。
- **账单** — `billingrule:<id>` 由规则保存/删除/切换共用；`billing:reprice` 独立。证据：`src/react-tests/billing-state.test.ts`、`src/react-tests/billing.test.tsx`。
- **审批** — `approval:<id>` 由通过/拒绝共用。选中操作显示 spinner，冲突的另一选项禁用。证据：`src/react-tests/approvals.test.tsx`。
- **使用情况维护与视角** — `usage:maintenance` 由清理/裁剪共用；`usage:perspective` 独立并忽略过期概览结果。维护操作继续使用现有清理对话框。证据：`src/react-tests/usage-store.test.ts`、`src/react-tests/usage-clear.test.tsx`。
- **会话路由** — `conversation:<id>` 由覆盖保存/清除与会话亲和重置共用。同一会话的操作和对话框关闭互斥；其他会话仍可独立操作。证据：`src/react-tests/usage-detail-store.test.ts`。
- **认证** — Worker 会话与 relay token 在 `src/lib/session.ts` 中拥有互相独立的 key 和互相独立的 store：Worker 会话使用 `session:authentication`，relay 管理 token 使用 `relay-session:authentication`。Busy 覆盖写操作和嵌套 bootstrap；重复登录不会导航或清除表单。Bootstrap/状态读取不会清除写操作 claim。证据：`src/react-tests/parity-routes.test.ts`、`src/react-tests/locale-theme.test.tsx`，以及 `frontend/e2e/foundation/browser-result.json` 中的渲染验收证据。

守卫本身由 `src/react-tests/async-action.test.tsx` 覆盖：key 同步申请，在必要的刷新期间保持，
失败后释放，已发布的 pending 映射被整体替换而不是就地修改。

## 读取与 UI 约定

搜索、列表、分页、bootstrap、状态和额度读取沿用现有加载状态及过期响应策略；它们不使用写操作
key。如果写操作必须等后续刷新完成，可将该读取放在写入回调内。本地编辑草稿不显示虚假的网络加载
状态。

独立异步读取保持自己的加载状态：控件在处理器运行期间显示自己的 pending 状态，只有当处理器
返回真实 Promise 时 spinner 才是诚实的。读取加载状态不能替代共享的领域写操作守卫。使用单一完成
路径和单一通知器；被拒绝的重复调用不得再次显示成功 toast。纯 UI action 改动保持公共 API 和生成
类型不变。
