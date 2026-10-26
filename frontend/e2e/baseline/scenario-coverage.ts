import { ROUTES, type ScenarioId } from '../parity.matrix.ts'

type CoverageEntry = {
  passed: boolean
  evidence: string
}

type BaselineEvidence = {
  captures?: Array<Record<string, any>>
  checks?: Record<string, any>
  mutations?: Array<Record<string, any>>
  residual?: Array<{ name: string; report: Record<string, any> }>
}

function lookup(value: any, path: string[]): any {
  return path.reduce((current, key) => current?.[key], value)
}

export function resolveCoverageEvidence(
  evidence: BaselineEvidence,
  reference: string,
): any {
  const [kind, name, ...path] = reference.split('/')
  if (kind === 'capture') {
    return evidence.captures?.find((capture) => capture.name === name)
  }
  if (kind === 'check') return lookup(evidence.checks, [name!, ...path])
  if (kind === 'mutation') {
    return evidence.mutations?.find((mutation) => mutation.name === name)
  }
  if (kind === 'residual') {
    const report = evidence.residual?.find((item) => item.name === name)?.report
    return lookup(report, path)
  }
  return undefined
}

function captureName(rowId: string, route: string): string {
  if (rowId === 'R-LOGIN') return 'auth_01_login_zh_dark_1440'
  if (rowId === 'R-RELAY-HOST')
    return 'relay_host_02_authenticated_zh_dark_1440'
  return `admin_zh_dark_1440${route.replaceAll('/', '_')}`
}

function mutation(
  evidence: BaselineEvidence,
  name: string,
): Record<string, any> | undefined {
  return evidence.mutations?.find((item) => item.name === name)
}

function residual(
  evidence: BaselineEvidence,
  name: string,
): Record<string, any> | undefined {
  return evidence.residual?.find((item) => item.name === name)?.report
}

function attempt11(
  evidence: BaselineEvidence,
  callback: string,
  check: string,
): any {
  return residual(evidence, callback)?.checks?.[check]
}

function captureNamed(
  evidence: BaselineEvidence,
  name: string,
): Record<string, any> | undefined {
  return evidence.captures?.find((capture) => capture.name === name)
}

function make(
  evidence: BaselineEvidence,
  reference: string,
  passed: boolean,
): CoverageEntry {
  return {
    passed:
      Boolean(passed) &&
      resolveCoverageEvidence(evidence, reference) !== undefined,
    evidence: reference,
  }
}

function mutationPassed(evidence: BaselineEvidence, name: string): boolean {
  const report = mutation(evidence, name)
  const writes = report?.writes ?? report?.requests ?? []
  return Boolean(report && !report.error && writes.length > 0)
}

function routeCapturePassed(
  evidence: BaselineEvidence,
  rowId: string,
  route: string,
): boolean {
  const capture = captureNamed(evidence, captureName(rowId, route))
  return Boolean(capture && capture.url === route)
}

function routeDialogPassed(evidence: BaselineEvidence, route: string): boolean {
  return Boolean(
    evidence.checks?.dialogs?.some(
      (dialog: Record<string, any>) =>
        dialog.route === route && dialog.opened && dialog.dialogVisible,
    ),
  )
}

function paginationResult(
  evidence: BaselineEvidence,
  rowId: string,
): { reference: string; passed: boolean } {
  if (rowId === 'R-API-KEYS') {
    const check = attempt11(
      evidence,
      'attempt11-missing',
      'R-API-KEYS:pagination',
    )
    const first = check?.firstIds ?? []
    const second = check?.secondIds ?? []
    return {
      reference: 'residual/attempt11-missing/checks/R-API-KEYS:pagination',
      passed:
        check?.query?.first === '10' &&
        check.query.rows === '10' &&
        first.length === 10 &&
        second.length === 2 &&
        check.changed &&
        !second.some((id: string) => first.includes(id)),
    }
  }
  if (rowId === 'R-REDACTION-RULES') {
    const check = attempt11(
      evidence,
      'attempt11-missing',
      'R-REDACTION-RULES:pagination',
    )
    const first = check?.firstPatterns ?? []
    const second = check?.secondPatterns ?? []
    return {
      reference:
        'residual/attempt11-missing/checks/R-REDACTION-RULES:pagination',
      passed:
        check?.query?.first === '10' &&
        check.query.rows === '10' &&
        first.length === 10 &&
        second.length === 2 &&
        check.changed &&
        !second.some((pattern: string) => first.includes(pattern)),
    }
  }
  if (rowId === 'R-USAGE-AI') {
    const check = evidence.checks?.usageRecords
    return {
      reference: 'check/usageRecords',
      passed: Boolean(check?.moved && check.pageTwoRows > 0),
    }
  }
  if (rowId === 'R-USAGE-MCP') {
    const check = residual(evidence, 'mcp-maintenance')?.checks?.pagination
    return {
      reference: 'residual/mcp-maintenance/checks/pagination',
      passed:
        check?.query?.first === '25' &&
        check.query.rows === '25' &&
        check.before === 25 &&
        check.after === 1 &&
        check.changed,
    }
  }
  if (rowId === 'R-BILLING') {
    const check = residual(evidence, 'four-slices')?.checks?.billing
    return {
      reference: 'residual/four-slices/checks/billing',
      passed: Boolean(check?.chargeChanged && check.chargeRequests?.length > 0),
    }
  }
  const name = {
    'R-AVAILABLE-MODELS': 'available-models',
    'R-USERS': 'users',
    'R-ENDPOINTS-UPSTREAMS': 'endpoints-upstreams',
    'R-ENDPOINTS-ROUTES': 'endpoints-routes',
    'R-MCP': 'mcp',
    'R-RELAYS': 'relays',
    'R-APPROVALS': 'approvals',
    'R-SETTINGS-CONFIG-ARCHIVE': 'config-archive',
  }[rowId]
  const check = name ? evidence.checks?.pagination?.[name] : undefined
  return {
    reference: `check/pagination/${name ?? rowId}`,
    passed: Boolean(check?.moved && check.changed && check.after > 0),
  }
}

function scenarioEvidence(
  evidence: BaselineEvidence,
  row: (typeof ROUTES)[number],
  scenario: ScenarioId,
): CoverageEntry {
  const route = row.route
  const rowId = row.id
  const capture = captureName(rowId, route)
  const visual = `capture/${capture}`
  const allowed = evidence.checks?.nonAdmin?.allowed?.some(
    (item: Record<string, any>) =>
      item.route === route && item.actual === route,
  )
  const denied = evidence.checks?.nonAdmin?.denied?.some(
    (item: Record<string, any>) =>
      item.route === route && item.actual === '/api-keys',
  )

  switch (scenario) {
    case 'route-render':
      return make(evidence, visual, routeCapturePassed(evidence, rowId, route))
    case 'nav-visibility':
      return make(
        evidence,
        'check/nonAdmin/allowed',
        Boolean(
          allowed ||
          (rowId === 'R-API-KEYS' &&
            evidence.checks?.nonAdmin?.landing === '/api-keys'),
        ),
      )
    case 'admin-denial':
      return make(evidence, 'check/nonAdmin/denied', Boolean(denied))
    case 'login':
      return make(
        evidence,
        'check/login',
        evidence.checks?.login?.landed === '/api-keys',
      )
    case 'logout':
      return make(
        evidence,
        'check/logout',
        evidence.checks?.logout?.landed === '/login',
      )
    case 'relay-independent-auth':
      return make(
        evidence,
        'check/relayHost',
        evidence.checks?.relayHost?.relaySession?.authenticated === true &&
          evidence.checks?.relayHost?.workerAfterRelay?.status === 401 &&
          String(
            evidence.checks?.relayHost?.workerRouteActual ?? '',
          ).startsWith('/login'),
      )
    case 'empty-state':
      if (rowId === 'R-AVAILABLE-MODELS') {
        const check = residual(evidence, 'advanced')?.checks?.emptyModels
        return make(
          evidence,
          'residual/advanced/checks/emptyModels',
          Boolean(check?.text && check.text.includes('没有数据')),
        )
      }
      if (rowId === 'R-USAGE-AI') {
        const empty = captureNamed(evidence, 'state_usage_empty_zh_dark_1440')
        return make(
          evidence,
          'capture/state_usage_empty_zh_dark_1440',
          Boolean(empty),
        )
      }
      if (rowId === 'R-USAGE-MCP') {
        const after = residual(evidence, 'mcp-maintenance')?.checks?.after
        return make(
          evidence,
          'residual/mcp-maintenance/checks/after',
          String(after ?? '').includes('暂无 MCP 调用记录'),
        )
      }
      break
    case 'loading-state':
      if (rowId === 'R-USAGE-AI') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-USAGE-AI:loading-state',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-USAGE-AI:loading-state',
          Boolean(
            check?.loadingVisible &&
            check.loaded &&
            check.overviewResponses?.includes(503) &&
            check.overviewResponses?.includes(200),
          ),
        )
      }
      if (rowId === 'R-ENDPOINTS-UPSTREAMS') {
        return make(
          evidence,
          'capture/state_endpoints_loading_zh_dark_1440',
          Boolean(
            captureNamed(evidence, 'state_endpoints_loading_zh_dark_1440'),
          ),
        )
      }
      break
    case 'error-state':
      if (rowId === 'R-LOGIN') {
        return make(
          evidence,
          'check/login',
          evidence.checks?.login?.invalidAlertVisible === true,
        )
      }
      if (rowId === 'R-ENDPOINTS-UPSTREAMS') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-ENDPOINTS-UPSTREAMS:error-state',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-ENDPOINTS-UPSTREAMS:error-state',
          Boolean(
            check?.failedRead &&
            check.errorVisible &&
            check.noStaleRows &&
            check.successRead &&
            check.rows > 0,
          ),
        )
      }
      if (rowId === 'R-APPROVALS') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-APPROVALS:error-state',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-APPROVALS:error-state',
          Boolean(
            check?.failed &&
            check.errorVisible &&
            check.noStaleRows &&
            check.retrySuccess &&
            check.rowCount > 0,
          ),
        )
      }
      if (rowId === 'R-USAGE-AI') {
        return make(
          evidence,
          'capture/state_usage_error_zh_dark_1440',
          Boolean(captureNamed(evidence, 'state_usage_error_zh_dark_1440')),
        )
      }
      break
    case 'pagination': {
      const result = paginationResult(evidence, rowId)
      return make(evidence, result.reference, result.passed)
    }
    case 'stale-response':
      if (rowId === 'R-USAGE-AI') {
        return make(
          evidence,
          'check/staleSearch',
          Boolean(
            evidence.checks?.staleSearch?.discardedSlowResponse &&
            evidence.checks?.staleDetail?.showedSecondRecord,
          ),
        )
      }
      if (rowId === 'R-REDACTION-PREVIEW') {
        return make(
          evidence,
          'check/redactionPreview',
          evidence.checks?.redactionPreview?.staleDiscarded === true,
        )
      }
      break
    case 'dialog-open':
      if (rowId === 'R-REDACTION-RULES') {
        return make(
          evidence,
          'check/deleteFlows/redaction',
          Boolean(
            evidence.checks?.deleteFlows?.redaction?.confirmIssuedSave &&
            evidence.checks.deleteFlows.redaction.deletes > 0,
          ),
        )
      }
      if (rowId === 'R-USAGE-AI') {
        return make(
          evidence,
          'check/staleDetail',
          evidence.checks?.staleDetail?.showedSecondRecord === true,
        )
      }
      if (rowId === 'R-USAGE-MCP') {
        const detail = residual(evidence, 'mcp-maintenance')?.checks?.detail
        return make(
          evidence,
          'residual/mcp-maintenance/checks/detail',
          Boolean(detail?.includes('请求载荷') && detail.includes('响应载荷')),
        )
      }
      if (rowId === 'R-APPROVALS') {
        return make(
          evidence,
          'mutation/approval-reject-resolved-filter',
          mutationPassed(evidence, 'approval-reject-resolved-filter'),
        )
      }
      return make(evidence, 'check/dialogs', routeDialogPassed(evidence, route))
    case 'mutation-create': {
      const name = {
        'R-API-KEYS': 'api-key-create',
        'R-USERS': 'user-create-update',
        'R-ENDPOINTS-UPSTREAMS': 'endpoint-create',
        'R-ENDPOINTS-ROUTES': 'model-route-create',
        'R-MCP': 'mcp-create',
        'R-RELAYS': 'relay-create-edit-secret',
        'R-BILLING': 'billing-price-rule-create-update',
      }[rowId]
      if (name)
        return make(
          evidence,
          `mutation/${name}`,
          mutationPassed(evidence, name),
        )
      break
    }
    case 'mutation-update':
      if (rowId === 'R-USERS')
        return make(
          evidence,
          'mutation/user-create-update',
          mutationPassed(evidence, 'user-create-update'),
        )
      if (rowId === 'R-ENDPOINTS-UPSTREAMS') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-ENDPOINTS-UPSTREAMS:update-failure-draft',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-ENDPOINTS-UPSTREAMS:update-failure-draft',
          Boolean(check?.samePayloadRetry && check.closedAfterRetry),
        )
      }
      if (rowId === 'R-ENDPOINTS-ROUTES') {
        const check = residual(evidence, 'form-payloads')?.checks?.targets
          ?.saved
        return make(
          evidence,
          'residual/form-payloads/checks/targets',
          Boolean(check?.body?.targets?.length),
        )
      }
      if (rowId === 'R-RELAY-HOST')
        return make(
          evidence,
          'check/relayRoleChange',
          Boolean(evidence.checks?.relayRoleChange?.write),
        )
      if (rowId === 'R-REDACTION-RULES')
        return make(
          evidence,
          'mutation/redaction-add-edit-save',
          mutationPassed(evidence, 'redaction-add-edit-save'),
        )
      if (rowId === 'R-REDACTION-PREVIEW')
        return make(
          evidence,
          'check/redactionPreview',
          Boolean(
            evidence.checks?.redactionPreview?.replacedEmail &&
            evidence.checks?.redactionPreview?.replacedSecret,
          ),
        )
      if (rowId === 'R-USAGE-AI')
        return make(
          evidence,
          'check/detailOverride',
          Boolean(
            evidence.checks?.detailOverride?.saveEnabled &&
            evidence.checks?.detailOverride?.clearEnabledAfterSave,
          ),
        )
      if (rowId === 'R-USAGE-MCP') {
        const check = residual(evidence, 'mcp-maintenance')?.checks
        return make(
          evidence,
          'residual/mcp-maintenance/checks/writes',
          Boolean(check?.writes?.length === 1),
        )
      }
      if (rowId === 'R-BILLING')
        return make(
          evidence,
          'mutation/billing-price-rule-create-update',
          mutationPassed(evidence, 'billing-price-rule-create-update'),
        )
      if (rowId === 'R-APPROVALS')
        return make(
          evidence,
          'mutation/approval-approve',
          mutationPassed(evidence, 'approval-approve'),
        )
      if (rowId === 'R-SETTINGS-REQUESTS')
        return make(
          evidence,
          'check/settingsSaves/requests',
          Boolean(evidence.checks?.settingsSaves?.requests?.saved),
        )
      if (rowId === 'R-SETTINGS-NETWORK')
        return make(
          evidence,
          'check/settingsSaves/network',
          Boolean(evidence.checks?.settingsSaves?.network?.saved),
        )
      if (rowId === 'R-SETTINGS-REVIEW')
        return make(
          evidence,
          'check/settingsSaves/review',
          Boolean(evidence.checks?.settingsSaves?.review?.saved),
        )
      if (rowId === 'R-SETTINGS-STORAGE')
        return make(
          evidence,
          'residual/settings-secrets/checks/storageKeep',
          Boolean(residual(evidence, 'settings-secrets')?.checks?.storageKeep),
        )
      if (rowId === 'R-SETTINGS-CACHE-ALERT')
        return make(
          evidence,
          'residual/settings-secrets/checks/cacheKeep',
          Boolean(residual(evidence, 'settings-secrets')?.checks?.cacheKeep),
        )
      break
    case 'mutation-delete': {
      const key = {
        'R-API-KEYS': 'apiKey',
        'R-USERS': 'user',
        'R-ENDPOINTS-UPSTREAMS': 'endpoint',
        'R-ENDPOINTS-ROUTES': 'modelRoute',
        'R-MCP': 'mcp',
        'R-RELAYS': 'relay',
        'R-BILLING': 'priceRule',
        'R-REDACTION-RULES': 'redaction',
      }[rowId]
      const check = key ? evidence.checks?.deleteFlows?.[key] : undefined
      const passed =
        key === 'priceRule'
          ? Boolean(check?.deleteIssued)
          : key === 'redaction'
            ? Boolean(check?.confirmIssuedSave && check.deletes > 0)
            : Boolean(
                check?.cancelIssuedDelete === false &&
                check?.confirmIssuedDelete,
              )
      return make(evidence, `check/deleteFlows/${key ?? rowId}`, passed)
    }
    case 'mutation-toggle':
      if (rowId === 'R-API-KEYS') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-API-KEYS:mutation-toggle',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-API-KEYS:mutation-toggle',
          Boolean(
            check?.statuses?.join(',') === '200,200' &&
            check.payloads?.[0]?.enabled === false &&
            check.payloads?.[1]?.enabled === true &&
            check.targetStates?.join(',') === 'false,true',
          ),
        )
      }
      if (rowId === 'R-ENDPOINTS-UPSTREAMS')
        return make(
          evidence,
          'mutation/endpoint-toggle-conflict-guard',
          mutationPassed(evidence, 'endpoint-toggle-conflict-guard'),
        )
      if (rowId === 'R-ENDPOINTS-ROUTES')
        return make(
          evidence,
          'mutation/model-route-toggle',
          mutationPassed(evidence, 'model-route-toggle'),
        )
      if (rowId === 'R-MCP')
        return make(
          evidence,
          'mutation/mcp-toggle',
          mutationPassed(evidence, 'mcp-toggle'),
        )
      if (rowId === 'R-REDACTION-RULES') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-REDACTION-RULES:mutation-toggle+mutation-failure-draft',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-REDACTION-RULES:mutation-toggle+mutation-failure-draft',
          Boolean(
            check?.toggled &&
            check.toggleBody?.rules?.[
              check.id?.replace('redaction-rule-', '')
            ] === false,
          ),
        )
      }
      if (rowId === 'R-BILLING') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-BILLING:mutation-toggle+mutation-conflict-guard',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-BILLING:mutation-toggle+mutation-conflict-guard',
          Boolean(
            check?.stateChanged &&
            check.writes?.at(-1)?.status === 200 &&
            check.writes?.at(-1)?.body?.enabled === false,
          ),
        )
      }
      break
    case 'mutation-reconnect':
      return make(
        evidence,
        'mutation/relay-reconnect',
        mutationPassed(evidence, 'relay-reconnect'),
      )
    case 'mutation-conflict-guard':
      if (rowId === 'R-API-KEYS') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-API-KEYS:mutation-conflict-guard',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-API-KEYS:mutation-conflict-guard',
          Boolean(
            check?.count === 1 && check.pendingDisabled && check.status === 503,
          ),
        )
      }
      if (rowId === 'R-ENDPOINTS-UPSTREAMS')
        return make(
          evidence,
          'check/conflictGuard',
          evidence.checks?.conflictGuard?.dispatched === 1,
        )
      if (rowId === 'R-ENDPOINTS-ROUTES') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-ENDPOINTS-ROUTES:mutation-conflict-guard',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-ENDPOINTS-ROUTES:mutation-conflict-guard',
          Boolean(
            check?.patchCount === 1 &&
            check.pendingDisabled &&
            check.status === 503,
          ),
        )
      }
      if (rowId === 'R-RELAYS') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-RELAYS:mutation-conflict-guard',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-RELAYS:mutation-conflict-guard',
          Boolean(
            check?.writeCount === 1 &&
            check.pendingDisabled &&
            check.status === 503,
          ),
        )
      }
      if (rowId === 'R-REDACTION-PREVIEW') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-REDACTION-PREVIEW:mutation-conflict-guard',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-REDACTION-PREVIEW:mutation-conflict-guard',
          Boolean(
            check?.count === 1 &&
            check.busy &&
            check.status === 503 &&
            check.failureVisible,
          ),
        )
      }
      if (rowId === 'R-BILLING') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-BILLING:mutation-toggle+mutation-conflict-guard',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-BILLING:mutation-toggle+mutation-conflict-guard',
          Boolean(
            check?.firstBurstCount === 1 &&
            check.pendingDisabled &&
            check.count === 2 &&
            check.writes?.[0]?.status === 503,
          ),
        )
      }
      if (rowId === 'R-APPROVALS') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-APPROVALS:mutation-conflict-guard',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-APPROVALS:mutation-conflict-guard',
          Boolean(
            check?.count === 1 &&
            check.alternativeDisabled &&
            check.status === 503,
          ),
        )
      }
      if (rowId === 'R-SETTINGS-REQUESTS') {
        const check = attempt11(
          evidence,
          'attempt11-settings',
          'R-SETTINGS-REQUESTS:mutation-conflict-guard',
        )
        return make(
          evidence,
          'residual/attempt11-settings/checks/R-SETTINGS-REQUESTS:mutation-conflict-guard',
          Boolean(
            check?.heldDisabled &&
            check.activeBeforeRelease === 1 &&
            check.serialized,
          ),
        )
      }
      break
    case 'mutation-failure-draft':
      if (rowId === 'R-API-KEYS')
        return make(
          evidence,
          'mutation/api-key-create-failure-draft',
          mutationPassed(evidence, 'api-key-create-failure-draft'),
        )
      if (rowId === 'R-USERS' || rowId === 'R-MCP') {
        const check = residual(evidence, 'domain-retries')?.checks?.[rowId]
        return make(
          evidence,
          `residual/domain-retries/checks/${rowId}`,
          Boolean(check?.draftRetained && check?.closedOnlyOnSuccess),
        )
      }
      if (rowId === 'R-ENDPOINTS-UPSTREAMS') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-ENDPOINTS-UPSTREAMS:mutation-failure-draft',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-ENDPOINTS-UPSTREAMS:mutation-failure-draft',
          Boolean(
            check?.retained &&
            check.errorVisible &&
            check.samePayloadRetry &&
            check.closedAfterRetry,
          ),
        )
      }
      if (rowId === 'R-RELAY-HOST')
        return make(
          evidence,
          'check/relayHostFailure',
          Boolean(
            evidence.checks?.relayHostFailure?.errorVisible &&
            evidence.checks?.relayHostFailure?.settingsDraft,
          ),
        )
      if (rowId === 'R-REDACTION-RULES') {
        const check = attempt11(
          evidence,
          'attempt11-missing',
          'R-REDACTION-RULES:mutation-toggle+mutation-failure-draft',
        )
        return make(
          evidence,
          'residual/attempt11-missing/checks/R-REDACTION-RULES:mutation-toggle+mutation-failure-draft',
          Boolean(
            check?.retained &&
            check.failureVisible &&
            check.samePayloadRetry &&
            check.writes?.join(',') === '200,503,200',
          ),
        )
      }
      if (rowId === 'R-SETTINGS-REQUESTS') {
        const callback = residual(evidence, 'attempt11-settings')?.checks
        const checks = [
          callback?.[
            'R-SETTINGS-REQUESTS:request-content-logging-failure-retry'
          ],
          callback?.['R-SETTINGS-REQUESTS:usage-retention-failure-retry'],
          callback?.['R-SETTINGS-REQUESTS:stream-delta-batching-failure-retry'],
        ]
        return make(
          evidence,
          'residual/attempt11-settings/checks',
          checks.every(
            (check) =>
              check?.draftRetained &&
              check.failureVisible &&
              check.failureThenRetry,
          ),
        )
      }
      if (rowId === 'R-SETTINGS-NETWORK')
        return make(
          evidence,
          'check/settingsSaves/networkFailure',
          Boolean(
            evidence.checks?.settingsSaves?.networkFailure?.draft &&
            evidence.checks.settingsSaves.networkFailure.errorVisible,
          ),
        )
      if (rowId === 'R-SETTINGS-REVIEW') {
        const check = attempt11(
          evidence,
          'attempt11-settings',
          'R-SETTINGS-REVIEW:mutation-failure-draft',
        )
        return make(
          evidence,
          'residual/attempt11-settings/checks/R-SETTINGS-REVIEW:mutation-failure-draft',
          Boolean(
            check?.retained && check.failureVisible && check.failureThenRetry,
          ),
        )
      }
      if (rowId === 'R-SETTINGS-STORAGE')
        return make(
          evidence,
          'residual/settings-secrets/checks/storageRetryPayload',
          Boolean(
            residual(evidence, 'settings-secrets')?.checks
              ?.storageFailureDraft &&
            residual(evidence, 'settings-secrets')?.checks?.storageRetryPayload,
          ),
        )
      if (rowId === 'R-SETTINGS-CACHE-ALERT')
        return make(
          evidence,
          'residual/settings-secrets/checks/cacheRetry',
          Boolean(
            residual(evidence, 'settings-secrets')?.checks?.cacheFailureDraft &&
            residual(evidence, 'settings-secrets')?.checks?.cacheRetry,
          ),
        )
      if (rowId === 'R-SETTINGS-CONFIG-ARCHIVE')
        return make(
          evidence,
          'check/archiveImport',
          Boolean(
            evidence.checks?.archiveImport?.fileRetained &&
            evidence.checks.archiveImport.failureVisible &&
            evidence.checks.archiveImport.passphraseClearedAfterFailure,
          ),
        )
      break
    case 'destructive-confirm':
      if (rowId === 'R-API-KEYS')
        return make(
          evidence,
          'check/deleteFlows/apiKey',
          Boolean(
            evidence.checks?.deleteFlows?.apiKey?.cancelIssuedDelete ===
              false &&
            evidence.checks?.deleteFlows?.apiKey?.confirmIssuedDelete,
          ),
        )
      if (rowId === 'R-USAGE-AI') {
        const item = mutation(evidence, 'usage-clear-cancel-confirm')
        return make(
          evidence,
          'mutation/usage-clear-cancel-confirm',
          Boolean(
            item &&
            !item.error &&
            item.writes?.some((request: string) =>
              request.includes('/request-records/clear'),
            ),
          ),
        )
      }
      if (rowId === 'R-USAGE-MCP') {
        const check = residual(evidence, 'mcp-maintenance')?.checks
        return make(
          evidence,
          'residual/mcp-maintenance/checks/cancelWrites',
          Boolean(check?.cancelWrites === 0 && check.writes?.length === 1),
        )
      }
      if (rowId === 'R-SETTINGS-CONFIG-ARCHIVE') {
        const check = residual(evidence, 'four-slices')?.checks?.archive
        return make(
          evidence,
          'residual/four-slices/checks/archive',
          Boolean(check?.cancelNoPost && check?.failed && check?.success),
        )
      }
      if (rowId === 'R-MCP')
        return make(
          evidence,
          'check/deleteFlows/mcp',
          Boolean(
            evidence.checks?.deleteFlows?.mcp?.cancelIssuedDelete === false &&
            evidence.checks?.deleteFlows?.mcp?.confirmIssuedDelete,
          ),
        )
      break
    case 'file-export':
      if (rowId === 'R-BILLING')
        return make(
          evidence,
          'check/csvDownloads',
          Boolean(
            evidence.checks?.csvDownloads?.details &&
            evidence.checks?.csvDownloads?.monthly,
          ),
        )
      if (rowId === 'R-SETTINGS-CONFIG-ARCHIVE')
        return make(
          evidence,
          'check/configExport',
          evidence.checks?.configExport?.downloadSuggested ===
            'prompt-ferry-config.pfce',
        )
      break
    case 'file-import': {
      const check = residual(evidence, 'four-slices')?.checks?.archive
      return make(
        evidence,
        'residual/four-slices/checks/archive',
        Boolean(
          check?.cancelNoPost &&
          check?.failed &&
          check?.success &&
          check?.auditRefreshed,
        ),
      )
    }
    case 'locale-switch':
    case 'theme-switch': {
      const check = attempt11(
        evidence,
        'attempt11-settings',
        'R-SETTINGS-GENERAL:locale-switch+theme-switch',
      )
      const passed =
        scenario === 'locale-switch'
          ? check?.localeStored === 'en-US' && check.settingsEnglish
          : check?.themeStored === 'light' && check.lightTheme
      return make(
        evidence,
        'residual/attempt11-settings/checks/R-SETTINGS-GENERAL:locale-switch+theme-switch',
        Boolean(passed),
      )
    }
    case 'responsive-width': {
      const captures = evidence.captures ?? []
      const routePrefix =
        rowId === 'R-RELAY-HOST' ? 'relay_host_' : `admin_zh_dark_`
      const rowRouteSlug = route.replaceAll('/', '_')
      const matching = captures.filter((item) =>
        rowId === 'R-RELAY-HOST'
          ? item.name.startsWith('relay_host_') && /_768$|_390$/.test(item.name)
          : item.name.startsWith(`${routePrefix}768${rowRouteSlug}`) ||
            item.name.startsWith(`${routePrefix}390${rowRouteSlug}`),
      )
      return make(
        evidence,
        matching[0] ? `capture/${matching[0].name}` : visual,
        matching.length > 0,
      )
    }
    case 'long-values': {
      const check = attempt11(evidence, 'attempt11-long-values', rowId)
      return make(
        evidence,
        `residual/attempt11-long-values/checks/${rowId}`,
        check?.passed === true,
      )
    }
    case 'zero-token-fold': {
      const checks = evidence.checks?.zeroTokenFold ?? []
      return make(
        evidence,
        'check/zeroTokenFold',
        checks.length === 2 &&
          checks.every(
            (check: Record<string, any>) =>
              check.foldedLabelPresent &&
              check.expandedRows > check.collapsedRows &&
              check.mixedZeroTokenVisible,
          ),
      )
    }
  }

  return make(evidence, visual, false)
}

export function buildScenarioCoverage(
  evidence: BaselineEvidence,
): Record<string, Record<string, CoverageEntry>> {
  return Object.fromEntries(
    ROUTES.map((row) => [
      row.id,
      Object.fromEntries(
        row.requiredScenarios.map((scenario) => [
          scenario,
          scenarioEvidence(evidence, row, scenario),
        ]),
      ),
    ]),
  )
}
