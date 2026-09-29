// Issue #589 P2c: cohesive OpenAI Platform locale block — upstream plan,
// ChatGPT OAuth login, Admin API Key and organization usage. Extracted from
// `endpoints.ts` to keep that module under the 400-line hard cap. Keys stay
// flat and are merged into the same locale object, so the `MessageKey` union
// and the runtime lookup by key are unchanged.
export const endpointOpenAiMessages = {
  'zh-CN': {
    endpointPlan: '上游计划',
    endpointPlanHint:
      'OpenAI 节点可选官方 API Key（Platform）或 ChatGPT Plus/Pro 订阅；订阅计划需要先完成 ChatGPT 登录。',
    endpointPlanPlatformApiKey: '官方 API Key（Platform）',
    endpointPlanChatgptSubscription: 'ChatGPT Plus/Pro 订阅',
    endpointPlanLoginRequired: '先完成下方 ChatGPT 登录，才能选择订阅计划。',
    endpointOAuth: 'ChatGPT 登录',
    endpointOAuthSaveFirst: '保存上游节点后，可在此完成 ChatGPT 登录。',
    endpointOAuthNotLoggedIn: '未登录',
    endpointOAuthLoggedIn: '已登录',
    endpointOAuthExpired: '凭据已过期',
    endpointOAuthExpiresAt: '凭据到期',
    endpointOAuthRefresh: '刷新凭据',
    endpointOAuthClear: '清除凭据',
    endpointOAuthQuota: '订阅额度',
    endpointOAuthQuotaHint:
      '仅展示 5 小时/周窗口用量，不参与路由权重，也不计入 Platform API 用量。',
    endpointOAuthQuotaPrimary: '5 小时窗口',
    endpointOAuthQuotaSecondary: '周窗口',
    endpointOAuthQuotaUnavailable: '额度暂不可用',
    endpointOAuthDeviceLogin: '设备码登录（Headless）',
    endpointOAuthDeviceStart: '开始登录',
    endpointOAuthDeviceHint:
      '在浏览器打开下方地址并输入设备码；确认后本页会自动完成登录。',
    endpointOAuthDeviceWaiting: '等待确认…',
    endpointOAuthBrowserLogin: '浏览器登录',
    endpointOAuthBrowserStart: '获取授权链接',
    endpointOAuthBrowserHint:
      '打开授权链接完成登录，浏览器会跳转到 http://localhost:1455/auth/callback；把地址栏中的完整跳转地址粘贴回来即可完成。',
    endpointOAuthAuthorizeUrl: '授权链接',
    endpointOAuthRedirectUrl: '跳转地址',
    endpointOAuthRedirectPlaceholder:
      'http://localhost:1455/auth/callback?code=...',
    endpointOAuthComplete: '完成登录',
    endpointOAuthCopy: '复制',
    endpointAdminApiKey: 'Admin API Key',
    endpointAdminApiKeyHint:
      '可选，仅 OpenAI 端点可用；与上方推理 API Key 分开保存，服务端使用，不会回显或写入日志。',
    endpointAdminApiKeyOptionalOnEdit: '留空则保持当前 Admin API Key',
    endpointAdminApiKeyClear: '清除已保存的 Admin API Key',
    endpointAdminApiKeyPendingClear: '保存后清除',
    endpointAdminApiKeyClearedOnSave: '保存后将清除',
    endpointAdminApiKeyKeepHint:
      '已保存的 Admin API Key 不会回显：直接保存保留原值；点击清除并保存后才会移除。',
    endpointOrganizationUsage: 'Platform 组织用量',
    endpointOrganizationUsageHint:
      '用 Admin API Key 读取其 OpenAI Platform 组织的用量与费用，统计 UTC 月初至今；这是组织级展示，不是 ChatGPT 订阅额度，也不参与路由权重、额度或推理 API Key 校验。',
    endpointOrganizationUsageSaveFirst: '保存端点后即可查询组织用量。',
    endpointOrganizationUsageRequiredKey:
      '先配置上方 Admin API Key 才能查询组织用量。',
    endpointOrganizationUsageLoad: '查询用量',
    endpointOrganizationUsageReload: '刷新用量',
    endpointOrganizationUsageInputTokens: '输入 tokens',
    endpointOrganizationUsageOutputTokens: '输出 tokens',
    endpointOrganizationUsageTotalTokens: '合计 tokens',
    endpointOrganizationUsageCost: '费用',
    endpointOrganizationUsagePeriod: '统计范围（UTC）：{start} – {end}',
    endpointOrganizationUsageFetchedAt: '读取时间',
    endpointOrganizationUsageCached: '60 秒缓存',
    endpointOrganizationUsageTruncated: '上游分页超过上限，合计为下限值',
  },
  'en-US': {
    endpointPlan: 'Upstream plan',
    endpointPlanHint:
      'OpenAI endpoints can use the official Platform API key or a ChatGPT Plus/Pro subscription; the subscription plan requires a completed ChatGPT login.',
    endpointPlanPlatformApiKey: 'Platform API key',
    endpointPlanChatgptSubscription: 'ChatGPT Plus/Pro subscription',
    endpointPlanLoginRequired:
      'Complete the ChatGPT login below before selecting the subscription plan.',
    endpointOAuth: 'ChatGPT login',
    endpointOAuthSaveFirst:
      'Save the endpoint first, then complete the ChatGPT login here.',
    endpointOAuthNotLoggedIn: 'Not logged in',
    endpointOAuthLoggedIn: 'Logged in',
    endpointOAuthExpired: 'Credential expired',
    endpointOAuthExpiresAt: 'Credential expires',
    endpointOAuthRefresh: 'Refresh credential',
    endpointOAuthClear: 'Clear credential',
    endpointOAuthQuota: 'Subscription quota',
    endpointOAuthQuotaHint:
      'Display-only 5-hour/weekly windows; they never affect routing weight or Platform API usage.',
    endpointOAuthQuotaPrimary: '5-hour window',
    endpointOAuthQuotaSecondary: 'Weekly window',
    endpointOAuthQuotaUnavailable: 'Quota is unavailable right now',
    endpointOAuthDeviceLogin: 'Device-code login (headless)',
    endpointOAuthDeviceStart: 'Start login',
    endpointOAuthDeviceHint:
      'Open the address below in a browser and enter the device code; this page completes the login once confirmed.',
    endpointOAuthDeviceWaiting: 'Waiting for confirmation…',
    endpointOAuthBrowserLogin: 'Browser login',
    endpointOAuthBrowserStart: 'Get authorize link',
    endpointOAuthBrowserHint:
      'Open the authorize link and finish the login; the browser lands on http://localhost:1455/auth/callback — paste the full address back here.',
    endpointOAuthAuthorizeUrl: 'Authorize link',
    endpointOAuthRedirectUrl: 'Redirect URL',
    endpointOAuthRedirectPlaceholder:
      'http://localhost:1455/auth/callback?code=...',
    endpointOAuthComplete: 'Complete login',
    endpointOAuthCopy: 'Copy',
    endpointAdminApiKey: 'Admin API key',
    endpointAdminApiKeyHint:
      'Optional, OpenAI endpoints only. Stored separately from the inference API keys above, used server-side, and never echoed or logged.',
    endpointAdminApiKeyOptionalOnEdit:
      'Leave blank to keep the current Admin API key',
    endpointAdminApiKeyClear: 'Clear saved Admin API key',
    endpointAdminApiKeyPendingClear: 'Cleared on save',
    endpointAdminApiKeyClearedOnSave: 'Will be cleared on save',
    endpointAdminApiKeyKeepHint:
      'The saved Admin API key is never echoed: saving keeps it, and it is removed only after Clear + Save.',
    endpointOrganizationUsage: 'Platform organization usage',
    endpointOrganizationUsageHint:
      'Uses the Admin API key to read its OpenAI Platform organization usage and spend for the UTC month to date. Organization-level display only: not the ChatGPT subscription quota, and never part of routing weight, quota, or inference API key checks.',
    endpointOrganizationUsageSaveFirst:
      'Save the endpoint to query organization usage.',
    endpointOrganizationUsageRequiredKey:
      'Configure the Admin API key above to query organization usage.',
    endpointOrganizationUsageLoad: 'Load usage',
    endpointOrganizationUsageReload: 'Refresh usage',
    endpointOrganizationUsageInputTokens: 'Input tokens',
    endpointOrganizationUsageOutputTokens: 'Output tokens',
    endpointOrganizationUsageTotalTokens: 'Total tokens',
    endpointOrganizationUsageCost: 'Spend',
    endpointOrganizationUsagePeriod: 'Period (UTC): {start} – {end}',
    endpointOrganizationUsageFetchedAt: 'Fetched at',
    endpointOrganizationUsageCached: '60s cache',
    endpointOrganizationUsageTruncated:
      'Upstream paging exceeded the cap; totals are a lower bound',
  },
} as const
