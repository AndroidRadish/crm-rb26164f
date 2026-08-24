//! 路由装配（CRM RB-26164F）

use axum::{
    routing::{get, post, put},
    Router,
};
use sqlx::PgPool;

use crate::handlers;

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/health", get(handlers::health::health))
        .route("/auth/login", post(handlers::auth::login))
        .route("/auth/me", get(handlers::auth::me))
        .route(
            "/users",
            post(handlers::auth::create_user).get(handlers::auth::list_users),
        )
        .route(
            "/customers",
            get(handlers::customers::list).post(handlers::customers::create),
        )
        .route(
            "/customers/check-duplicate",
            post(handlers::customers::check_duplicate_handler),
        )
        .route(
            "/customers/:id",
            get(handlers::customers::get)
                .put(handlers::customers::update)
                .delete(handlers::customers::delete),
        )
        .route("/customers/:id/claim", post(handlers::customers::claim))
        .route("/customers/:id/release", post(handlers::customers::release))
        .route(
            "/customers/:id/activities",
            get(handlers::activities::list).post(handlers::activities::create),
        )
        .route(
            "/customers/:id/tasks",
            get(handlers::tasks::list_by_customer).post(handlers::tasks::create),
        )
        .route("/tasks", get(handlers::tasks::list))
        .route(
            "/tasks/:id",
            put(handlers::tasks::update).delete(handlers::tasks::delete),
        )
        .route(
            "/stages",
            get(handlers::stages::list).post(handlers::stages::create),
        )
        .route(
            "/stages/:id",
            put(handlers::stages::update).delete(handlers::stages::delete),
        )
        .route(
            "/pipeline",
            get(handlers::pipeline::list).post(handlers::pipeline::create),
        )
        .route(
            "/pipeline/:id",
            put(handlers::pipeline::update).delete(handlers::pipeline::delete),
        )
        .route("/pipeline/kanban", get(handlers::pipeline::kanban))
        .route("/pipeline/funnel", get(handlers::pipeline::funnel))
        .route("/import", post(handlers::import::upload))
        .route("/import/template", get(handlers::import::template))
        .route("/import/batches", get(handlers::import::list_batches))
        .route("/import/batches/:id", get(handlers::import::get_batch))
        .route("/audit", get(handlers::audit::list))
        .route(
            "/export/customers.csv",
            get(handlers::import::export_customers_csv),
        )
        .route("/settings", get(handlers::settings::get_settings).put(handlers::settings::put_settings))
        .route("/dashboard", get(handlers::pipeline::dashboard))
        .route("/entitlements", get(handlers::entitlements::list_mine))
        .route("/admin/tenants", get(handlers::entitlements::admin_list_tenants))
        .route(
            "/admin/tenants/:id/entitlements",
            get(handlers::entitlements::admin_get_entitlements)
                .put(handlers::entitlements::admin_put_entitlements),
        )
}
