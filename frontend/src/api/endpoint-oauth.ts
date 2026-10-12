import {
  oauthBrowserComplete,
  oauthBrowserStart,
  oauthClear,
  oauthDevicePoll,
  oauthDeviceStart,
  oauthRefresh,
  oauthStatus,
  tokenPlanUsage,
} from '@/generated/admin-api'
import type {
  EndpointOAuthStatusResponse,
  OAuthBrowserStartResponse,
  OAuthDeviceStartResponse,
  OAuthLoginResponse,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'

export async function getEndpointOAuthStatus(
  endpointId: string,
): Promise<EndpointOAuthStatusResponse> {
  return expectData(
    await oauthStatus<true>(withData({ path: { endpoint_id: endpointId } })),
  )
}

export async function getEndpointTokenPlanUsage(
  endpointId: string,
): Promise<TokenPlanUsageResponse> {
  return expectData(
    await tokenPlanUsage<true>(withData({ path: { endpoint_id: endpointId } })),
  )
}

export async function startEndpointDeviceLogin(
  endpointId: string,
): Promise<OAuthDeviceStartResponse> {
  return expectData(
    await oauthDeviceStart<true>(
      withData({ path: { endpoint_id: endpointId } }),
    ),
  )
}

export async function pollEndpointDeviceLogin(
  endpointId: string,
  flowId: string,
): Promise<OAuthLoginResponse> {
  return expectData(
    await oauthDevicePoll<true>(
      withData({
        path: { endpoint_id: endpointId },
        body: { flow_id: flowId },
      }),
    ),
  )
}

export async function startEndpointBrowserLogin(
  endpointId: string,
): Promise<OAuthBrowserStartResponse> {
  return expectData(
    await oauthBrowserStart<true>(
      withData({ path: { endpoint_id: endpointId } }),
    ),
  )
}

export async function completeEndpointBrowserLogin(
  endpointId: string,
  flowId: string,
  redirectUrl: string,
): Promise<OAuthLoginResponse> {
  return expectData(
    await oauthBrowserComplete<true>(
      withData({
        path: { endpoint_id: endpointId },
        body: { flow_id: flowId, redirect_url: redirectUrl },
      }),
    ),
  )
}

export async function refreshEndpointOAuthToken(
  endpointId: string,
): Promise<EndpointOAuthStatusResponse> {
  return expectData(
    await oauthRefresh<true>(withData({ path: { endpoint_id: endpointId } })),
  )
}

export async function clearEndpointOAuthToken(
  endpointId: string,
): Promise<void> {
  await oauthClear<true>(withData({ path: { endpoint_id: endpointId } }))
}
