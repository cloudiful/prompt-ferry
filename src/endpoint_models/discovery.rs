use std::future::Future;

use anyhow::Result;

use crate::db::RouteConfig;

use super::{EndpointModelCache, EndpointModelSnapshot};

pub async fn discover_route_for_model<F, Fut>(
    cache: &EndpointModelCache,
    visible_routes: &[RouteConfig],
    fallback_route: Option<&RouteConfig>,
    model: &str,
    fetcher: F,
) -> Option<RouteConfig>
where
    F: Fn(&RouteConfig) -> Fut,
    Fut: Future<Output = Result<EndpointModelSnapshot>>,
{
    let mut matches = Vec::new();
    for route in visible_routes {
        let Ok(snapshot) = cache.load_or_fetch(route, &fetcher).await else {
            continue;
        };
        if snapshot.is_some_and(|snapshot| snapshot.contains(model)) {
            matches.push(route.clone());
        }
    }
    choose_discovered_route(&matches, fallback_route)
}

pub fn choose_discovered_route(
    matches: &[RouteConfig],
    fallback_route: Option<&RouteConfig>,
) -> Option<RouteConfig> {
    if matches.is_empty() {
        return None;
    }
    if let Some(fallback) = fallback_route
        && matches
            .iter()
            .any(|route| route.route_id == fallback.route_id)
    {
        return Some(fallback.clone());
    }
    if matches.len() == 1 {
        return Some(matches[0].clone());
    }
    None
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        config::NativeApi,
        db::{RouteConfig, RouteSelectionReason},
    };
    use uuid::Uuid;

    fn route(id: Uuid, base_url: &str) -> RouteConfig {
        RouteConfig {
            route_id: id,
            user_id: 7,
            base_url: base_url.to_string(),
            api_key: "secret".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Chat,
            upstream_model: None,
            responses_continuation_policy: crate::db::ResponsesContinuationPolicy::ForceReplay,
            model_route_rule_id: None,
            route_selection_reason: RouteSelectionReason::Default,
        }
    }

    #[tokio::test]
    async fn unique_match_is_selected_when_fallback_does_not_match() {
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let fallback = route(Uuid::new_v4(), "https://fallback.example.com");
        let target = route(Uuid::new_v4(), "https://target.example.com");
        let visible = vec![fallback.clone(), target.clone()];

        let selected = discover_route_for_model(
            &cache,
            &visible,
            Some(&fallback),
            "deepseek-chat",
            |route| {
                let snapshot = if route.route_id == target.route_id {
                    EndpointModelSnapshot::from_model_ids(["deepseek-chat"])
                } else {
                    EndpointModelSnapshot::from_model_ids(["gpt-4.1-mini"])
                };
                async move { Ok(snapshot) }
            },
        )
        .await;

        assert_eq!(selected.map(|route| route.route_id), Some(target.route_id));
    }

    #[test]
    fn ambiguous_matches_keep_existing_fallback_behavior() {
        let fallback = route(Uuid::new_v4(), "https://fallback.example.com");
        let other = route(Uuid::new_v4(), "https://other.example.com");

        assert!(choose_discovered_route(&[fallback, other], None).is_none());
    }

    #[test]
    fn fallback_wins_when_it_also_advertises_model() {
        let fallback = route(Uuid::new_v4(), "https://fallback.example.com");
        let other = route(Uuid::new_v4(), "https://other.example.com");

        let selected = choose_discovered_route(&[fallback.clone(), other], Some(&fallback));

        assert_eq!(
            selected.map(|route| route.route_id),
            Some(fallback.route_id)
        );
    }
}
