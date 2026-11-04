//! HTTP routes. Everything under `/api` is described in OpenAPI (Swagger UI at `/api/docs`).

pub mod auth;
pub mod bot;
pub mod chat;
pub mod commands;
pub mod library;
pub mod live;
pub mod queue;
pub mod settings;
pub mod users;

use axum::{
    http::{header, HeaderValue},
    routing::any,
    Router,
};
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use utoipa::{
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityRequirement, SecurityScheme},
    Modify, OpenApi,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa_swagger_ui::SwaggerUi;

use crate::{error::AppError, google, state::AppState};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "streaming-player",
        description = "Song requests for live streams. Chat bots use `/api/bot/{command}`. \
                       Every other API takes the web session cookie or `Authorization: Bearer` \
                       with an access token from `/api/auth/token` or a personal API token \
                       (`spat_…`), and checks one permission point."
    ),
    modifiers(&BearerAuth),
    tags(
        (name = "bot", description = "For chat bots (Nightbot, StreamElements, Fossabot, Streamer.bot…)"),
        (name = "auth", description = "Sign-in. Every API takes a session cookie or `Authorization: Bearer` with an access token (`/api/auth/token`) or a personal API token (`spat_…`)"),
        (name = "users", description = "Accounts, roles and permission points"),
        (name = "live", description = "WebSocket and media for player pages"),
        (name = "public", description = "No sign-in needed"),
    )
)]
struct ApiDoc;

/// Lets Swagger UI send `Authorization: Bearer …` (an access token or an API token).
struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT access token or spat_ API token")
                    .build(),
            ),
        );
        openapi.security = Some(vec![SecurityRequirement::new(
            "bearer",
            Vec::<String>::new(),
        )]);
    }
}

pub fn openapi_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(live::health))
        .routes(routes!(live::meta))
        .routes(routes!(live::ws))
        .routes(routes!(live::media_source))
        .routes(routes!(live::media_stream))
        .routes(routes!(live::public_state))
        .routes(routes!(live::public_requests))
        .routes(routes!(auth::state))
        .routes(routes!(auth::setup))
        .routes(routes!(auth::login))
        .routes(routes!(auth::logout))
        .routes(routes!(auth::change_password))
        .routes(routes!(auth::register))
        .routes(routes!(auth::token))
        .routes(routes!(auth::refresh))
        .routes(routes!(auth::revoke))
        .routes(routes!(auth::list_api_tokens, auth::create_api_token))
        .routes(routes!(auth::delete_api_token))
        .routes(routes!(google::start))
        .routes(routes!(google::callback))
        .routes(routes!(users::list, users::create))
        .routes(routes!(users::update, users::remove))
        .routes(routes!(users::audit_log))
        .routes(routes!(users::list_roles, users::create_role))
        .routes(routes!(users::update_role, users::delete_role))
        .routes(routes!(queue::get, queue::add))
        .routes(routes!(queue::remove))
        .routes(routes!(queue::move_to))
        .routes(routes!(queue::reorder))
        .routes(routes!(queue::play_now))
        .routes(routes!(queue::clear))
        .routes(routes!(queue::pause))
        .routes(routes!(queue::resume))
        .routes(routes!(queue::skip))
        .routes(routes!(queue::volume))
        .routes(routes!(library::list, library::add))
        .routes(routes!(library::update, library::remove))
        .routes(routes!(library::enqueue))
        .routes(routes!(library::export))
        .routes(routes!(library::import))
        .routes(routes!(library::history))
        .routes(routes!(library::stats))
        .routes(routes!(library::blocklist, library::block))
        .routes(routes!(library::unblock))
        .routes(routes!(library::blocked_songs))
        .routes(routes!(library::recent_events))
        .routes(routes!(commands::list, commands::create))
        .routes(routes!(commands::update, commands::remove))
        .routes(routes!(commands::reset))
        .routes(routes!(settings::get, settings::update))
        .routes(routes!(settings::rotate_key))
        .routes(routes!(settings::youtube_key))
        .routes(routes!(settings::system))
        .routes(routes!(settings::update_ytdlp))
        .routes(routes!(settings::export_backup, settings::import_backup))
        .routes(routes!(chat::info))
        .routes(routes!(chat::twitch_device))
        .routes(routes!(chat::twitch_forget))
        .routes(routes!(chat::restart))
        .routes(routes!(bot::run))
}

pub fn router(state: AppState) -> Router {
    let (api, openapi) = openapi_router().split_for_parts();

    // The web app is a single-page app: unknown paths get index.html and its router.
    let static_dir = state.config.static_dir.clone();
    let spa = ServeDir::new(&static_dir).fallback(ServeFile::new(static_dir.join("index.html")));

    Router::new()
        .merge(api)
        .merge(SwaggerUi::new("/api/docs").url("/api/openapi.json", openapi))
        .route(
            "/api/{*rest}",
            any(|| async { AppError::not_found("找不到這個 API") }),
        )
        .fallback_service(spa)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        // The YouTube player refuses to run (error 153) when the page sends no Referer,
        // so keep the browser default instead of `same-origin` / `no-referrer`.
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
