mod game;
mod session;
mod utils;

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    Router,
    response::{IntoResponse, Response},
    routing::{get, patch, put},
};
use maud::{Markup, html};
use pico_args::Arguments;
use time::Duration;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_cookies::CookieManagerLayer;
use tower_http::compression::CompressionLayer;

use crate::{
    game::{BoardBuilder, Point, ShipDefinition},
    session::{SessionManager, SessionOptionExt, Store},
    utils::{
        assets::asset_handler,
        errors::{AnyhowWebExt, WebResult},
        htmx::{HtmxRedirect, HtmxTarget},
        shutdown,
    },
};

async fn game_handler(
    sessions: session::SessionManager,
    target: HtmxTarget,
) -> WebResult<Response> {
    // TODO: redirect to new game page instead of error
    let session = sessions.current().require()?;
    let board = &session.board;

    let cell: Point = target
        .parse()
        .context("Invalid cell definition")
        .map_err(|e| e.client_error())?;

    let display_diff = board.hit(cell).await?;

    if board.is_win().await {
        sessions.delete(session).await;
        Ok(HtmxRedirect::to("/game/win").into_response())
    } else {
        Ok(display_diff.render().await.into_response())
    }
}

async fn new_game_handler(sessions: SessionManager) -> WebResult<impl IntoResponse> {
    let session = sessions.create(
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

async fn continue_game_handler(sessions: SessionManager) -> WebResult<impl IntoResponse> {
    let session = sessions.current().require()?;
    Ok(session.board.render().await)
}

fn page(modifier: &'static str, html: Markup) -> Markup {
    html!(
        (maud::DOCTYPE)
        html lang="ru" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                link rel="stylesheet" href ="/vendor/normalize.min.css";
                link rel="stylesheet" href="/ui.css";

                link rel="icon" type="image/png" sizes="16x16" href="/favicon/16x16.png";
                link rel="icon" type="image/png" sizes="32x32" href="/favicon/32x32.png";
                link rel="icon" type="image/png" sizes="96x96" href="/favicon/96x96.png";

                meta name="htmx-config" content={r#"{"defaultSwapStyle": "outerHTML"}"#};
                script src="/vendor/htmx.min.js" {}
            };

            body {
                #screen class=(modifier) {
                    #display class=(modifier) {
                        (html)
                    }
                }
            }
        }
    )
}

async fn page_app(sessions: SessionManager) -> impl IntoResponse {
    page(
        "waves",
        html!({
            .btn.menu
                hx-put={"/game"}
                hx-target="body"
                hx-swap="innerHTML"
                {"Начать игру"};

            @if sessions.current_exists() {
                .btn.menu
                    hx-get={"/game"}
                    hx-target="body"
                    hx-swap="innerHTML"
                    {"Продолжить игру"};
            }}
        ),
    )
}

pub async fn page_win() -> Markup {
    page(
        "waves",
        html!({
            #win-text {"Победа!"}
            a #win-exit href="/" {
                .btn.exit  { "Выход" }
            }
        }),
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

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber)?;

    let mut args = Arguments::from_env();
    let listener = listener_from_args(&mut args).await?;

    let store = Arc::new(Store::new(Duration::days(1)));
    let store = store.with_cleanup();

    let router = Router::new()
        .route("/", get(page_app))
        .route("/game/win", get(page_win))
        //
        .route("/game", get(continue_game_handler))
        .route("/game", put(new_game_handler))
        .route("/game", patch(game_handler))
        //
        .route("/{*path}", get(asset_handler))
        .layer(
            ServiceBuilder::new()
                .layer(CompressionLayer::new())
                .layer(CookieManagerLayer::new()),
        )
        .with_state(store.clone());

    Ok(axum::serve(listener, router)
        .with_graceful_shutdown(shutdown::signal())
        .await
        .unwrap())
}
