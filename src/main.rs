mod game;
mod store;
mod utils;

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use axum::{
    Router, extract,
    response::IntoResponse,
    routing::{get, post},
};
use maud::html;
use pico_args::Arguments;
use serde::Deserialize;
use time::Duration;
use tokio::{net::TcpListener, signal, sync::RwLock};
use tower::ServiceBuilder;
use tower_cookies::{CookieManagerLayer, Cookies};
use tower_http::compression::CompressionLayer;

use crate::{
    game::{BoardBuilder, Point, ShipDefinition},
    store::{Store, StoreAccessor},
    utils::{
        assets::asset_handler,
        errors::{AnyhowWebExt, WebResult},
    },
};

type Dyn<T> = Arc<RwLock<T>>;

// TODO: simplify
#[derive(Deserialize)]
struct RenderRequestData {
    hit: Point,
}

async fn game_handler(
    store: extract::State<StoreAccessor>,
    cookies: Cookies,
    extract::Query(data): extract::Query<RenderRequestData>,
) -> WebResult<impl IntoResponse> {
    // TODO: redirect to new game page instead of error
    let session = store
        .get_session(&cookies)
        .ok_or(anyhow!("Board not found").client_error())?;

    let board = &session.board;
    board.hit(data.hit).await?;

    if board.is_win().await {
        store.remove_session(session, &cookies).await;
        Ok(game::ui::render_win())
    } else {
        Ok(board.render().await)
    }
}

async fn new_game_handler(
    store: extract::State<StoreAccessor>,
    cookies: Cookies,
) -> WebResult<impl IntoResponse> {
    let session = store.new_session(
        &cookies,
        BoardBuilder::square(10)
            .random(&[
                ShipDefinition::new("Линкор", 4, 1),
                ShipDefinition::new("Крейсер", 3, 2),
                ShipDefinition::new("Эсминец", 2, 3),
                ShipDefinition::new("Торпеда", 1, 4),
            ])
            .await?,
    )?;

    let board = &session.board;
    Ok(board.render().await)
}

async fn app_handler() -> impl IntoResponse {
    html!(
        (maud::DOCTYPE)
        html lang="ru" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                link rel="stylesheet" href ="vendor/normalize.min.css";
                link rel="stylesheet" href="ui.css";

                link rel="icon" type="image/png" sizes="16x16" href="/favicon/16x16.png";
                link rel="icon" type="image/png" sizes="32x32" href="/favicon/32x32.png";
                link rel="icon" type="image/png" sizes="96x96" href="/favicon/96x96.png";

                script src="vendor/htmx.min.js" {}
            };

            body {
                #container { // TODO: Hx-Redirect instead
                    #screen .waves { // TODO: partial screen updates
                        #new-game-btn
                            hx-post={"/game/new"}
                            hx-swap="outerHtml"
                            hx-target="#container"
                            {"Начать игру"}
                    }
                }
            }
        }
    )
}

async fn listener_from_args(args: &mut Arguments) -> Result<TcpListener> {
    let addr = args
        .opt_value_from_str("--bind")?
        .unwrap_or("0.0.0.0:8080".to_string());

    tracing::info!("Listening on http://{addr}");

    TcpListener::bind(addr)
        .await
        .context("Failed to bind listener")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutting down");
}

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber)?;

    let mut args = Arguments::from_env();
    let listener = listener_from_args(&mut args).await?;

    let store = Arc::new(Store::new(Duration::days(1)));
    let store = store.with_cleanup();

    let router = Router::new()
        .route("/", get(app_handler))
        .route("/game/new", post(new_game_handler))
        .route("/game", post(game_handler))
        .route("/{*path}", get(asset_handler))
        .layer(
            ServiceBuilder::new()
                .layer(CompressionLayer::new())
                .layer(CookieManagerLayer::new()),
        )
        .with_state(store.clone());

    Ok(axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap())
}
