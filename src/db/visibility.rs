//! Row visibility for the safe catalog reads.
//!
//! One rule decides every catalog row: a shared row carries the `admin`
//! scope without an owner, an owned row carries the `user` scope with the
//! viewer as its owner. Anything else — an unknown scope, a missing owner, or
//! an owner that is not the viewer — is not visible to that viewer.
//!
//! The repository queries build their predicate with [`visible_condition`]
//! for the count and for the paged rows, so both sides always see the same
//! set. The SQLite endpoint table has no ownership columns: every endpoint
//! there is shared, so its queries ignore the viewer entirely.

use sea_orm::sea_query::{Condition, Expr, ExprTrait, IntoIden};

/// How one visible catalog row is scoped for the current viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CatalogVisibility {
    /// An administrator-owned resource every authenticated user may read.
    Shared,
    /// A user-scoped resource the viewer owns.
    Owned,
}

/// The one visibility rule, applied to a stored scope and owner.
///
/// `None` means the row is not visible to this viewer.
pub(crate) fn catalog_visibility(
    scope: &str,
    owner_user_id: Option<i64>,
    viewer: i64,
) -> Option<CatalogVisibility> {
    match scope {
        "admin" if owner_user_id.is_none() => Some(CatalogVisibility::Shared),
        "user" if owner_user_id == Some(viewer) => Some(CatalogVisibility::Owned),
        _ => None,
    }
}

/// The SQL predicate for the same rule.
///
/// The shared branch is `scope = 'admin' AND owner IS NULL` and the owned
/// branch is `scope = 'user' AND owner = <viewer>`; a malformed row matches
/// neither branch and stays hidden.
pub(crate) fn visible_condition<S, O>(scope: S, owner: O, viewer: i64) -> Condition
where
    S: IntoIden + Clone,
    O: IntoIden + Clone,
{
    Condition::any()
        .add(
            Expr::col(scope.clone())
                .eq("user")
                .and(Expr::col(owner.clone()).eq(viewer)),
        )
        .add(Expr::col(scope).eq("admin").and(Expr::col(owner).is_null()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn a_shared_row_is_visible_to_every_viewer() {
        assert_eq!(
            catalog_visibility("admin", None, 7),
            Some(CatalogVisibility::Shared)
        );
        assert_eq!(
            catalog_visibility("admin", None, 99),
            Some(CatalogVisibility::Shared)
        );
    }

    #[test]
    fn an_owned_row_is_visible_only_to_its_owner() {
        assert_eq!(
            catalog_visibility("user", Some(7), 7),
            Some(CatalogVisibility::Owned)
        );
        assert_eq!(catalog_visibility("user", Some(7), 8), None);
        assert_eq!(catalog_visibility("user", None, 7), None);
    }

    #[test]
    fn a_malformed_row_is_never_visible() {
        // An admin-scope row with an owner, an unknown scope, and an empty
        // scope all stay hidden.
        assert_eq!(catalog_visibility("admin", Some(7), 7), None);
        assert_eq!(catalog_visibility("shared", None, 7), None);
        assert_eq!(catalog_visibility("", None, 7), None);
    }

    /// The count and the paged rows must use one predicate, so both sides
    /// count and page the same visible set.
    #[test]
    fn the_count_and_the_page_queries_share_one_predicate() {
        use crate::db::entities::postgres::provider_endpoints::{Column, Entity};

        let count = Query::select()
            .expr_as(
                Expr::col(Column::EndpointId).count(),
                sea_orm::sea_query::Alias::new("total"),
            )
            .from(Entity)
            .cond_where(visible_condition(Column::Scope, Column::OwnerUserId, 7))
            .to_owned();
        let page = Query::select()
            .column(Column::EndpointId)
            .from(Entity)
            .cond_where(visible_condition(Column::Scope, Column::OwnerUserId, 7))
            .to_owned();
        let (count_sql, _) = count.build(PostgresQueryBuilder);
        let (page_sql, _) = page.build(PostgresQueryBuilder);
        let count_where = count_sql
            .split_once(" WHERE ")
            .expect("the count query carries a predicate")
            .1;
        let page_where = page_sql
            .split_once(" WHERE ")
            .expect("the page query carries a predicate")
            .1;
        assert_eq!(count_where, page_where);
        assert!(count_where.contains("scope"));
        assert!(count_where.contains("owner_user_id"));
    }
}
