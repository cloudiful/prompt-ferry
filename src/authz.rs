//! One process-wide authorization engine and the prompt-ferry adapter schema.
//!
//! The engine is built once during startup and shared by the admin state and
//! the relay state. This module only maps the existing prompt-ferry authority
//! onto explicit capabilities; it does not add a public scope surface or a
//! second permission store.

use std::collections::BTreeSet;

use cloudiful_authz::{
    Action, Attenuation, AuthorizerConfig, AuthzRequest, Capability, CredentialContext,
    DomainHierarchy, DomainId, FactsRevision, Grant, InitError, PrincipalId, PrincipalKind,
    PrincipalRef, ResolvedRoles, ResourceId, ResourceKind, ResourceRef, RoleId, RoleSpec,
    ServiceId, Subject,
};
pub use cloudiful_authz::{Authorizer, Decision};

/// The single application domain. Both the admin and relay surfaces authorize
/// against this domain; it carries no tenant hierarchy in this phase.
pub(crate) const DOMAIN: &str = "prompt-ferry";
const AUDIENCE: &str = "prompt-ferry";
const FACTS_REVISION: u64 = 1;

const ROLE_ADMIN: &str = "admin";
const ROLE_USER: &str = "user";

const CONTROL_ACTION: &str = "control";
const INFERENCE_RESOURCE: &str = "inference";
const INFERENCE_ACTION: &str = "request";

/// The existing admin control groups.
///
/// The admin role enumerates each group with the dedicated `control` action,
/// so a normal user capability on the same resource name can never satisfy an
/// administrator check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    User,
    ClientKey,
    Endpoint,
    Oauth,
    SessionRouting,
    Billing,
    ConfigAudit,
    ConfigExport,
    ConfigImport,
    ModelRoute,
    Node,
    Quota,
    Settings,
    Usage,
}

impl ControlAction {
    const ALL: [Self; 14] = [
        Self::User,
        Self::ClientKey,
        Self::Endpoint,
        Self::Oauth,
        Self::SessionRouting,
        Self::Billing,
        Self::ConfigAudit,
        Self::ConfigExport,
        Self::ConfigImport,
        Self::ModelRoute,
        Self::Node,
        Self::Quota,
        Self::Settings,
        Self::Usage,
    ];

    fn resource(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ClientKey => "client_key",
            Self::Endpoint => "endpoint",
            Self::Oauth => "oauth",
            Self::SessionRouting => "session_routing",
            Self::Billing => "billing",
            Self::ConfigAudit => "config_audit",
            Self::ConfigExport => "config_export",
            Self::ConfigImport => "config_import",
            Self::ModelRoute => "model_route",
            Self::Node => "node",
            Self::Quota => "quota",
            Self::Settings => "settings",
            Self::Usage => "usage",
        }
    }

    fn capability(self) -> Capability {
        Capability {
            resource: ResourceKind::new(self.resource()),
            action: Action::new(CONTROL_ACTION),
        }
    }
}

/// The existing authenticated-user business capabilities.
///
/// Ownership and administrator visibility stay in the existing SQL filters;
/// these actions only decide whether the authenticated session may reach the
/// business route at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAction {
    ClientKeyManage,
    ModelRead,
    UsageRead,
    UsageManage,
    McpOauthManage,
    McpServerManage,
    SessionRoutingRead,
    SettingsRead,
    SettingsManage,
    BridgeRead,
}

impl UserAction {
    const ALL: [Self; 10] = [
        Self::ClientKeyManage,
        Self::ModelRead,
        Self::UsageRead,
        Self::UsageManage,
        Self::McpOauthManage,
        Self::McpServerManage,
        Self::SessionRoutingRead,
        Self::SettingsRead,
        Self::SettingsManage,
        Self::BridgeRead,
    ];

    fn capability(self) -> Capability {
        let (resource, action) = match self {
            Self::ClientKeyManage => ("client_key", "manage"),
            Self::ModelRead => ("model", "read"),
            Self::UsageRead => ("usage", "read"),
            Self::UsageManage => ("usage", "manage"),
            Self::McpOauthManage => ("mcp_oauth", "manage"),
            Self::McpServerManage => ("mcp_server", "manage"),
            Self::SessionRoutingRead => ("session_routing", "read"),
            Self::SettingsRead => ("settings", "read"),
            Self::SettingsManage => ("settings", "manage"),
            Self::BridgeRead => ("bridge", "read"),
        };
        Capability {
            resource: ResourceKind::new(resource),
            action: Action::new(action),
        }
    }
}

fn admin_role() -> RoleSpec {
    RoleSpec {
        id: RoleId::new(ROLE_ADMIN),
        capabilities: ControlAction::ALL
            .into_iter()
            .map(ControlAction::capability)
            .collect(),
    }
}

fn user_role() -> RoleSpec {
    let mut capabilities = UserAction::ALL
        .into_iter()
        .map(UserAction::capability)
        .collect::<BTreeSet<_>>();
    capabilities.insert(Capability {
        resource: ResourceKind::new(INFERENCE_RESOURCE),
        action: Action::new(INFERENCE_ACTION),
    });
    RoleSpec {
        id: RoleId::new(ROLE_USER),
        capabilities,
    }
}

fn config() -> AuthorizerConfig {
    AuthorizerConfig {
        facts_revision: FactsRevision(FACTS_REVISION),
        domains: vec![DomainId::new(DOMAIN)],
        roles: vec![admin_role(), user_role()],
        assignments: Vec::new(),
        global_assignments: Vec::new(),
        hierarchy: DomainHierarchy::default(),
        public_read: BTreeSet::new(),
    }
}

/// Build the one engine owned by the current process.
pub(crate) async fn build() -> Result<Authorizer, InitError> {
    Authorizer::new(config()).await
}

fn principal(user_id: i64) -> PrincipalRef {
    PrincipalRef {
        kind: PrincipalKind::Human,
        id: PrincipalId::new(user_id.to_string()),
    }
}

fn resolved_roles(user_id: i64, is_admin: bool) -> ResolvedRoles {
    let mut roles = BTreeSet::from([RoleId::new(ROLE_USER)]);
    if is_admin {
        roles.insert(RoleId::new(ROLE_ADMIN));
    }
    ResolvedRoles {
        principal: principal(user_id),
        domain: DomainId::new(DOMAIN),
        roles,
    }
}

fn request(
    user_id: i64,
    capability: Capability,
    resource_id: Option<&str>,
    credential: Option<CredentialContext>,
) -> AuthzRequest {
    let domain = DomainId::new(DOMAIN);
    AuthzRequest {
        subject: Subject::Principal(principal(user_id)),
        domain: domain.clone(),
        resource: ResourceRef {
            kind: capability.resource,
            id: resource_id.map(ResourceId::new),
            domain,
        },
        action: capability.action,
        audience: ServiceId::new(AUDIENCE),
        facts_revision: FactsRevision(FACTS_REVISION),
        credential,
        server: None,
    }
}

/// Decide one existing admin control action for a live session user.
pub(crate) fn authorize_control(
    authorizer: &Authorizer,
    user_id: i64,
    is_admin: bool,
    action: ControlAction,
) -> Decision {
    authorizer.authorize_with_roles(
        &request(user_id, action.capability(), None, None),
        &resolved_roles(user_id, is_admin),
    )
}

/// Decide one existing authenticated-user business action.
pub(crate) fn authorize_user(
    authorizer: &Authorizer,
    user_id: i64,
    is_admin: bool,
    action: UserAction,
) -> Decision {
    authorizer.authorize_with_roles(
        &request(user_id, action.capability(), None, None),
        &resolved_roles(user_id, is_admin),
    )
}

/// Decide an inference request for the actual snapshot user and route.
///
/// The credential is the route-bound inference grant derived from the applied
/// snapshot. It can only satisfy `inference.request` on that exact route, so
/// the same key never reaches a control or usage capability.
pub(crate) fn authorize_inference(
    authorizer: &Authorizer,
    user_id: i64,
    route_id: &str,
) -> Decision {
    let capability = Capability {
        resource: ResourceKind::new(INFERENCE_RESOURCE),
        action: Action::new(INFERENCE_ACTION),
    };
    let grant = Grant {
        domain: DomainId::new(DOMAIN),
        resource: capability.resource.clone(),
        resource_id: Some(ResourceId::new(route_id)),
        action: capability.action.clone(),
    };
    let credential = CredentialContext {
        principal: principal(user_id),
        audience: ServiceId::new(AUDIENCE),
        grants: Attenuation::Grants(BTreeSet::from([grant])),
    };
    authorizer.authorize_with_roles(
        &request(user_id, capability, Some(route_id), Some(credential)),
        &resolved_roles(user_id, false),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloudiful_authz::DenyReason;

    async fn authorizer() -> Authorizer {
        build().await.expect("authorizer builds")
    }

    #[tokio::test]
    async fn admin_control_actions_allow_admin_and_deny_a_normal_user() {
        let authorizer = authorizer().await;
        for action in ControlAction::ALL {
            assert!(
                authorize_control(&authorizer, 1, true, action).is_allowed(),
                "admin must keep {action:?}"
            );
            assert_eq!(
                authorize_control(&authorizer, 1, false, action),
                Decision::Deny(DenyReason::NoSubjectAuthority),
                "a normal user must not reach {action:?}"
            );
        }
    }

    #[tokio::test]
    async fn user_business_actions_allow_every_authenticated_role() {
        let authorizer = authorizer().await;
        for action in UserAction::ALL {
            assert!(
                authorize_user(&authorizer, 7, false, action).is_allowed(),
                "user must keep {action:?}"
            );
            assert!(
                authorize_user(&authorizer, 7, true, action).is_allowed(),
                "admin must keep user action {action:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_inference_key_never_reaches_control_or_user_business_actions() {
        let authorizer = authorizer().await;
        assert!(authorize_inference(&authorizer, 7, "route-a").is_allowed());
        assert_eq!(
            authorize_control(&authorizer, 7, false, ControlAction::ClientKey),
            Decision::Deny(DenyReason::NoSubjectAuthority),
            "an inference-only key never gains client-key control"
        );
        assert_eq!(
            authorize_control(&authorizer, 7, false, ControlAction::Usage),
            Decision::Deny(DenyReason::NoSubjectAuthority),
            "an inference-only key never gains usage control"
        );
    }

    #[tokio::test]
    async fn a_user_control_request_uses_the_live_role_facts() {
        let authorizer = authorizer().await;
        let resolved = resolved_roles(3, false);
        let control = ControlAction::User.capability();
        assert_eq!(
            authorizer.authorize_with_roles(&request(3, control, None, None), &resolved),
            Decision::Deny(DenyReason::NoSubjectAuthority)
        );
    }

    #[tokio::test]
    async fn an_inference_credential_cannot_authorize_user_business_actions() {
        let authorizer = authorizer().await;
        let credential = CredentialContext {
            principal: principal(7),
            audience: ServiceId::new(AUDIENCE),
            grants: Attenuation::Grants(BTreeSet::from([Grant {
                domain: DomainId::new(DOMAIN),
                resource: ResourceKind::new(INFERENCE_RESOURCE),
                resource_id: Some(ResourceId::new("route-a")),
                action: Action::new(INFERENCE_ACTION),
            }])),
        };
        for action in [
            UserAction::UsageRead,
            UserAction::BridgeRead,
            UserAction::McpOauthManage,
        ] {
            assert_eq!(
                authorizer.authorize_with_roles(
                    &request(7, action.capability(), None, Some(credential.clone())),
                    &resolved_roles(7, false)
                ),
                Decision::Deny(DenyReason::CredentialGrantMissing),
                "inference credential must not authorize {action:?}"
            );
        }
    }

    #[tokio::test]
    async fn bridge_and_mcp_oauth_reads_allow_authenticated_users_and_admins() {
        let authorizer = authorizer().await;
        for action in [UserAction::BridgeRead, UserAction::McpOauthManage] {
            assert!(authorize_user(&authorizer, 7, false, action).is_allowed());
            assert!(authorize_user(&authorizer, 7, true, action).is_allowed());
        }
    }

    #[tokio::test]
    async fn an_inference_request_without_the_route_grant_is_denied() {
        let authorizer = authorizer().await;
        let capability = Capability {
            resource: ResourceKind::new(INFERENCE_RESOURCE),
            action: Action::new(INFERENCE_ACTION),
        };
        let credential = CredentialContext {
            principal: principal(7),
            audience: ServiceId::new(AUDIENCE),
            grants: Attenuation::Grants(BTreeSet::from([Grant {
                domain: DomainId::new(DOMAIN),
                resource: capability.resource.clone(),
                resource_id: Some(ResourceId::new("route-a")),
                action: capability.action.clone(),
            }])),
        };
        assert_eq!(
            authorizer.authorize_with_roles(
                &request(7, capability, Some("route-b"), Some(credential)),
                &resolved_roles(7, false)
            ),
            Decision::Deny(DenyReason::CredentialGrantMissing)
        );
    }
}
