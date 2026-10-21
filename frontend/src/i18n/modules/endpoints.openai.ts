// Issue #589 P2c: cohesive OpenAI Platform locale block — upstream plan,
// ChatGPT OAuth login and organization usage. Extracted from
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
  },
} as const
