# 前端异步操作

[English](frontend-async-actions.md)

远程写操作使用共享的 `useAsyncAction` 守卫。守卫由 store 或领域 action 工厂按互斥资源持有一份，该资源的所有入口都使用相同 key。key 已被占用时返回 `{ executed: false }`；UI 处理器只有在 `executed` 为 true 时才执行成功副作用，包括通知、导航、关闭对话框和重置草稿。

守卫应覆盖完整写操作及必要刷新。等待前捕获记录身份、所选用户和提交载荷。将 pending 状态传给冲突控件：禁用冲突输入和关闭操作，保持无关资源可用，仅让实际发起操作的控件显示 spinner。错误保持可见，草稿可重试。共享 store action 外不创建组件级守卫。

## 操作覆盖

- **Endpoint 与模型路由** — `endpoint:<id>` 覆盖 endpoint 保存/删除/切换，以及 OAuth 设备/浏览器登录启动、每次轮询、完成、刷新和清除。`modelroute:<id>` 覆盖路由保存/删除/切换。探测使用独立的 `networktest:<domain>:<id>`。Pending 状态传递到行和 endpoint 对话框。OAuth 控件通过页面和对话框接收 store 所有的回调；每次设备轮询独立申请 key，人工授权期间不占用 key。证据：`endpoint-actions.gates.test.ts`、`endpoint-oauth.test.ts`、`use-endpoint-oauth-actions.test.ts`、`endpoint-oauth-status.test.ts`、`endpoint-oauth-device-flow.test.ts`、`endpoint-oauth-browser-flow.test.ts` 及 endpoint 页面/SFC 测试。
- **MCP 服务** — `mcp:<id>` 覆盖保存/删除，`mcp:create` 覆盖创建；目录探测使用 `mcptest:<id>`。Pending 状态传递到服务行和对话框。目录编辑是本地草稿。证据：`mcp-store.test.ts`、`record-actions.gates.test.ts`、`record-actions.lifecycle.test.ts`、`record-actions.sfc.test.ts`。
- **用户** — `user:<id>` 覆盖保存/删除/密码重置；`user:create` 覆盖创建。Pending 状态传递到用户行/对话框及重置控件。证据：`users-store.test.ts` 及 record-action 守卫、生命周期和 SFC 测试。
- **客户端 key** — `clientkey:<id>` 覆盖标签/保存/删除/切换；创建使用已捕获所选用户对应的 key。Pending 状态传递到 key 行/对话框。证据：`api-keys-store.test.ts` 及 record-action 守卫、生命周期和 SFC 测试。
- **Relay** — `relay:<id>` 覆盖保存/删除/重连。Owner pending 状态传递到行操作和对话框；写入及刷新期间禁用字段和关闭操作，仅发起操作显示 spinner。证据：`relays-store.test.ts`、`relay-actions.lifecycle.test.ts`、`relay-actions.sfc.test.ts`、`relay-session-guard.test.ts`。
- **Relay Host** — `host:local` 将角色/设置/重启视作同一 host 资源。共享 pending 状态会禁用互斥控件，仅发起操作显示 spinner；角色/设置完成时保留提交后编辑的字段。证据：`relay-host-role.test.ts`、`relay-host-actions.lifecycle.test.ts`、`relay-actions.sfc.test.ts`、`relay-session-guard.test.ts`。
- **设置** — `setting:<resource>` 覆盖各设置区；请求内容记录与使用保留期共用 `setting:rawRetention`。保留期写入时两表单均禁用，仅发起方显示 spinner。证据：`settings-store.test.ts`、`settings-actions.gates.test.ts`、`settings-actions.lifecycle.test.ts`、`settings-actions.sfc.test.ts`。
- **脱敏** — `redaction:rules` 覆盖保存，`redaction:preview` 覆盖非持久化预览。两者反馈独立；过期预览响应会被忽略。证据：`redaction-store.test.ts` 及 remaining-action 生命周期/SFC 测试。
- **配置归档** — 预览/应用共用向导 key；显式导出有独立读取反馈。向导保留确认、文件校验和可重试草稿状态。证据：`config-archive-actions.test.ts` 及 remaining-action 生命周期/SFC 测试。
- **账单** — `billingrule:<id>` 由规则保存/删除/切换共用；`billing:reprice` 独立。证据：`billing-store.test.ts`、`billing-actions.lifecycle.test.ts`、`billing-actions.sfc.test.ts`。
- **审批** — `approval:<id>` 由通过/拒绝共用。选中操作显示 spinner，冲突的另一选项禁用。证据：`approvals-store.test.ts` 及 remaining-action 守卫/生命周期/SFC 测试。
- **使用情况维护与视角** — `usage:maintenance` 由清理/裁剪共用；`usage:perspective` 独立并忽略过期概览结果。维护操作继续使用现有清理对话框。证据：`request-records-store.test.ts`、`usage-actions.gates.test.ts`、`usage-actions.lifecycle.test.ts`、`usage-actions.sfc.test.ts`。
- **会话路由** — `conversation:<id>` 由覆盖保存/清除与会话亲和重置共用。同一会话的操作和对话框关闭互斥；其他会话仍可独立操作。证据：`usage-actions.gates.test.ts`、`usage-actions.lifecycle.test.ts`、`usage-actions.sfc.test.ts`、`request-record-detail.test.ts`。
- **认证** — Worker 登录/退出与 Relay token 登录/退出在各自 session store 中使用独立 key。Busy 覆盖写操作和嵌套 bootstrap；重复登录不会导航或清除表单。Bootstrap/状态读取不会清除写操作 claim。证据：`session-login.lifecycle.test.ts`、`relay-session-guard.test.ts`。

## 读取与 UI 约定

搜索、列表、分页、bootstrap、状态和额度读取沿用现有加载状态及过期响应策略；它们不使用写操作 key。如果写操作必须等后续刷新完成，可将该读取放在写入回调内。本地编辑草稿不显示虚假的网络加载状态。

独立异步读取按钮在处理器返回实际 Promise 时可以使用 Nuxt UI 的 `loading-auto`。多个入口可能触发同一写操作时，它不能替代领域守卫。使用单一完成路径和单一通知器；被拒绝的重复调用不得再次显示成功 toast。纯 UI action 改动保持公共 API 和生成类型不变。
