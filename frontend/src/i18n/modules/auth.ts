export const authMessages = {
  'zh-CN': {
    hidePassword: '隐藏密码',
    invalidCredentials: '登录名或密码错误',
    login: '登录',
    loginName: '登录名',
    loginNameRequired: '请输入登录名',
    logout: '退出登录',
    password: '密码',
    passwordRequired: '请输入密码',
    sessionLoadFailed: '无法加载会话，请重试',
    showPassword: '显示密码',
    setupTitle: '初始化管理员',
    setupIntro: '首次运行需要创建管理员账号；创建后使用该账号登录。',
    setupMasterSecret: '部署主密钥',
    setupMasterSecretRequired: '请输入部署主密钥',
    setupSubmit: '创建管理员',
    setupTransportWarning:
      '主密钥只通过当前连接提交一次。请使用 HTTPS 或可信的本地连接完成初始化。',
    setupSharedDatabaseNote: '管理员账号保存在共享数据库，而不是当前节点。',
    setupDisplayNameHelp: '留空时使用登录名。',
    setupUnavailable: '初始化不可用',
    setupUnavailableDesc:
      '当前实例无法完成初始化。请确认数据库可用后重试，不要重复初始化。',
    setupStatusFailed: '无法读取初始化状态',
    setupStatusFailedDesc:
      '尚未确认实例是否已初始化，因此不会显示登录或初始化表单。请重试。',
  },
  'en-US': {
    hidePassword: 'Hide password',
    invalidCredentials: 'Invalid login name or password',
    login: 'Login',
    loginName: 'Login name',
    loginNameRequired: 'Please enter login name',
    logout: 'Logout',
    password: 'Password',
    passwordRequired: 'Please enter password',
    sessionLoadFailed: 'Could not load session. Please try again.',
    showPassword: 'Show password',
    setupTitle: 'Set up administrator',
    setupIntro:
      'Create the administrator account on first run, then sign in with it.',
    setupMasterSecret: 'Deployment master secret',
    setupMasterSecretRequired: 'Please enter the deployment master secret',
    setupSubmit: 'Create administrator',
    setupTransportWarning:
      'The master secret is submitted once over this connection. Use HTTPS or a trusted local connection for setup.',
    setupSharedDatabaseNote:
      'The administrator account is stored in the shared database, not on this node.',
    setupDisplayNameHelp: 'Defaults to the login name.',
    setupUnavailable: 'Setup unavailable',
    setupUnavailableDesc:
      'This instance cannot complete setup. Make sure the database is available and retry without reinitializing.',
    setupStatusFailed: 'Could not read setup status',
    setupStatusFailedDesc:
      'It is not confirmed whether this instance is initialized, so neither login nor setup is shown. Please retry.',
  },
} as const
