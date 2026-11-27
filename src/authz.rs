//! One process-wide authorization engine and the prompt-ferry adapter schema.
//!
//! The engine is built once during startup and shared by the admin state and
//! the relay state. This module only maps the existing prompt-ferry authority
//! onto explicit capabilities; it does not add a public scope surface or a
//! second permission store.

use std::collections::BTreeSet;

use cloudiful_authz::{
    Attenuation, Authority, AuthorizerConfig, AuthzRequest, Capability, CredentialContext,
    DomainId, FactsRevision, Grant, InitError, PrincipalRef, ResolvedRoles, ResourceId,
    ResourceRef, RoleId, RoleSpec, ServiceId,
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
        Capability::new(self.resource(), CONTROL_ACTION)
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
    /// Issue #99 P1: read the shared/own upstream endpoint catalog without
    /// any control action. The row visibility stays in the repository
    /// filters; this action only admits the authenticated session.
    EndpointRead,
    /// Issue #99 P1: read the shared/own model-route catalog.
    ModelRouteRead,
    /// Issue #99 P1: read the cached quota of a visible endpoint.
    QuotaRead,
}

impl UserAction {
    const ALL: [Self; 13] = [
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
        Self::EndpointRead,
        Self::ModelRouteRead,
        Self::QuotaRead,
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
            // Issue #99 P1: the catalog reads are their own `read` actions on
            // the endpoint/model_route/quota resources. The admin control
            // groups keep the dedicated `control` action on the same
            // resources, so a catalog read never satisfies a control check.
            Self::EndpointRead => ("endpoint", "read"),
            Self::ModelRouteRead => ("model_route", "read"),
            Self::QuotaRead => ("quota", "read"),
        };
        Capability::new(resource, action)
    }
}

/// One typed authorization operation for the single adapter check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operation<'a> {
    Control(ControlAction),
    User(UserAction),
    Inference { route_id: &'a str },
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
    capabilities.insert(Capability::new(INFERENCE_RESOURCE, INFERENCE_ACTION));
    RoleSpec {
        id: RoleId::new(ROLE_USER),
        capabilities,
    }
}

fn config() -> AuthorizerConfig {
    AuthorizerConfig::new(
        FactsRevision(FACTS_REVISION),
        [DOMAIN],
        [admin_role(), user_role()],
    )
}

/// Build the one engine owned by the current process.
pub(crate) async fn build() -> Result<Authorizer, InitError> {
    Authorizer::new(config()).await
}

fn request(
    capability: Capability,
    resource_id: Option<&str>,
    credential: Option<CredentialContext>,
) -> AuthzRequest {
    AuthzRequest {
        resource: ResourceRef {
            kind: capability.resource,
            id: resource_id.map(ResourceId::new),
            domain: DomainId::new(DOMAIN),
        },
        action: capability.action,
        audience: ServiceId::new(AUDIENCE),
        credential,
        server: None,
    }
}

/// The single adapter entry for an existing control, user or inference action.
///
/// The caller supplies the already verified user id and current administrator
/// fact, or the applied snapshot route for inference. The inference credential
/// is the exact route-bound `inference.request` grant, so a key never gains a
/// control or user business capability.
pub(crate) fn check(
    authorizer: &Authorizer,
    user_id: i64,
    is_admin: bool,
    operation: Operation<'_>,
) -> Decision {
    let principal = PrincipalRef::human(user_id.to_string());
    let domain = DomainId::new(DOMAIN);
    let mut roles = BTreeSet::from([RoleId::new(ROLE_USER)]);
    let (capability, resource_id, credential) = match operation {
        Operation::Control(action) => {
            if is_admin {
                roles.insert(RoleId::new(ROLE_ADMIN));
            }
            (action.capability(), None, None)
        }
        Operation::User(action) => {
            if is_admin {
                roles.insert(RoleId::new(ROLE_ADMIN));
            }
            (action.capability(), None, None)
        }
        Operation::Inference { route_id } => {
            // A managed key always resolves to the user role, never the
            // session administrator fact of the user it is bound to.
            let capability = Capability::new(INFERENCE_RESOURCE, INFERENCE_ACTION);
            let credential = CredentialContext {
                principal: principal.clone(),
                audience: ServiceId::new(AUDIENCE),
                grants: Attenuation::grants([Grant::new(
                    DOMAIN,
                    INFERENCE_RESOURCE,
                    INFERENCE_ACTION,
                )
                .with_resource_id(route_id)]),
            };
            (capability, Some(route_id), Some(credential))
        }
    };
    let request = request(capability, resource_id, credential);
    let resolved = ResolvedRoles::new(principal, domain, roles);
    authorizer.authorize(&request, Authority::Registered(&resolved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloudiful_authz::DenyReason;

    async fn authorizer() -> Authorizer {
        build().await.expect("authorizer builds")
    }

    fn session_roles(user_id: i64, is_admin: bool) -> ResolvedRoles {
        let mut roles = BTreeSet::from([RoleId::new(ROLE_USER)]);
        if is_admin {
            roles.insert(RoleId::new(ROLE_ADMIN));
        }
        ResolvedRoles::new(PrincipalRef::human(user_id.to_string()), DOMAIN, roles)
    }

    fn inference_credential(user_id: i64, route_id: &str) -> CredentialContext {
        CredentialContext {
            principal: PrincipalRef::human(user_id.to_string()),
            audience: ServiceId::new(AUDIENCE),
            grants: Attenuation::grants([
                Grant::new(DOMAIN, INFERENCE_RESOURCE, INFERENCE_ACTION).with_resource_id(route_id)
            ]),
        }
    }

    #[tokio::test]
    async fn admin_control_actions_allow_admin_and_deny_a_normal_user() {
        let authorizer = authorizer().await;
        for action in ControlAction::ALL {
            assert!(
                check(&authorizer, 1, true, Operation::Control(action)).is_allowed(),
                "admin must keep {action:?}"
            );
            assert_eq!(
                check(&authorizer, 1, false, Operation::Control(action)),
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
                check(&authorizer, 7, false, Operation::User(action)).is_allowed(),
                "user must keep {action:?}"
            );
            assert!(
                check(&authorizer, 7, true, Operation::User(action)).is_allowed(),
                "admin must keep user action {action:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_inference_key_never_reaches_control_or_user_business_actions() {
        let authorizer = authorizer().await;
        assert!(
            check(
                &authorizer,
                7,
                false,
                Operation::Inference {
                    route_id: "route-a"
                }
            )
            .is_allowed()
        );
        assert_eq!(
            check(
                &authorizer,
                7,
                false,
                Operation::Control(ControlAction::ClientKey)
            ),
            Decision::Deny(DenyReason::NoSubjectAuthority),
            "an inference-only key never gains client-key control"
        );
        assert_eq!(
            check(
                &authorizer,
                7,
                false,
                Operation::Control(ControlAction::Usage)
            ),
            Decision::Deny(DenyReason::NoSubjectAuthority),
            "an inference-only key never gains usage control"
        );
    }

    #[tokio::test]
    async fn control_operations_use_the_live_role_facts() {
        let authorizer = authorizer().await;
        assert_eq!(
            check(
                &authorizer,
                3,
                false,
                Operation::Control(ControlAction::User)
            ),
            Decision::Deny(DenyReason::NoSubjectAuthority)
        );
        assert!(
            check(
                &authorizer,
                3,
                true,
                Operation::Control(ControlAction::User)
            )
            .is_allowed()
        );
    }

    #[tokio::test]
    async fn an_inference_credential_cannot_authorize_user_business_actions() {
        let authorizer = authorizer().await;
        let credential = inference_credential(7, "route-a");
        for action in [
            UserAction::UsageRead,
            UserAction::BridgeRead,
            UserAction::McpOauthManage,
        ] {
            let request = request(action.capability(), None, Some(credential.clone()));
            assert_eq!(
                authorizer.authorize(&request, Authority::Registered(&session_roles(7, false))),
                Decision::Deny(DenyReason::CredentialGrantMissing),
                "inference credential must not authorize {action:?}"
            );
        }
    }

    #[tokio::test]
    async fn bridge_and_mcp_oauth_reads_allow_authenticated_users_and_admins() {
        let authorizer = authorizer().await;
        for action in [UserAction::BridgeRead, UserAction::McpOauthManage] {
            assert!(check(&authorizer, 7, false, Operation::User(action)).is_allowed());
            assert!(check(&authorizer, 7, true, Operation::User(action)).is_allowed());
        }
    }

    #[tokio::test]
    async fn catalog_read_actions_allow_admin_and_normal_users_and_deny_control_actions() {
        let authorizer = authorizer().await;
        for action in [
            UserAction::EndpointRead,
            UserAction::ModelRouteRead,
            UserAction::QuotaRead,
        ] {
            assert!(
                check(&authorizer, 7, false, Operation::User(action)).is_allowed(),
                "a normal user must reach {action:?}"
            );
            assert!(
                check(&authorizer, 7, true, Operation::User(action)).is_allowed(),
                "an admin must reach {action:?}"
            );
        }
        // The catalog reads never satisfy a control check: the endpoint,
        // model_route and quota control groups keep their own action.
        for action in [
            ControlAction::Endpoint,
            ControlAction::ModelRoute,
            ControlAction::Quota,
        ] {
            assert_eq!(
                check(&authorizer, 7, false, Operation::Control(action)),
                Decision::Deny(DenyReason::NoSubjectAuthority),
                "a normal user must not reach {action:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_anonymous_subject_reaches_no_business_action() {
        let authorizer = authorizer().await;
        // An anonymous session carries no role, so every business read is
        // denied instead of falling back to a default capability.
        for action in [
            UserAction::EndpointRead,
            UserAction::ModelRouteRead,
            UserAction::QuotaRead,
        ] {
            let request = request(action.capability(), None, None);
            assert_eq!(
                authorizer.authorize(
                    &request,
                    Authority::Registered(&ResolvedRoles::new(
                        PrincipalRef::human("0".to_string()),
                        DOMAIN,
                        BTreeSet::<RoleId>::new(),
                    ))
                ),
                Decision::Deny(DenyReason::NoSubjectAuthority),
                "an anonymous subject must not reach {action:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_inference_credential_cannot_authorize_the_catalog_reads() {
        let authorizer = authorizer().await;
        let credential = inference_credential(7, "route-a");
        for action in [
            UserAction::EndpointRead,
            UserAction::ModelRouteRead,
            UserAction::QuotaRead,
        ] {
            let request = request(action.capability(), None, Some(credential.clone()));
            assert_eq!(
                authorizer.authorize(&request, Authority::Registered(&session_roles(7, false))),
                Decision::Deny(DenyReason::CredentialGrantMissing),
                "an inference-only credential must not authorize {action:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_inference_request_without_the_route_grant_is_denied() {
        let authorizer = authorizer().await;
        let request = request(
            Capability::new(INFERENCE_RESOURCE, INFERENCE_ACTION),
            Some("route-b"),
            Some(inference_credential(7, "route-a")),
        );
        assert_eq!(
            authorizer.authorize(&request, Authority::Registered(&session_roles(7, false))),
            Decision::Deny(DenyReason::CredentialGrantMissing)
        );
    }
}
