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
use time::{Duration, OffsetDateTime};
use tokio::{net::TcpListener, sync::RwLock};
use tower::ServiceBuilder;
use tower_cookies::{Cookie, CookieManagerLayer, Cookies};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

use crate::{
    game::{BoardBuilder, Point, ShipDefinition},
    store::StoreAccessor,
    utils::{errors::Fallible, scheduler},
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
) -> Fallible<impl IntoResponse> {
    let store = store.read().await;

    // TODO: redirect to new game page
    let board_id = cookies
        .get("board")
        .ok_or(anyhow!("Board not found. Most likely it expired."))?
        .value()
        .parse()
        .map_err(|_| anyhow!("Invalid board ID."))?;

    let board = match store.get_board(board_id).await {
        Some(board) => board,
        None => return Err(anyhow!("Board not found.").into()),
    }
    .lock()
    .await;

    board.hit(data.hit).await?;

    Ok(board.render().await)
}

async fn new_game_handler(
    store: extract::State<StoreAccessor>,
    cookies: Cookies,
) -> Fallible<impl IntoResponse> {
    let mut store = store.write().await;

    let now = OffsetDateTime::now_utc();
    let expires = now + Duration::days(1);

    let (id, board) = store
        .new_board(
            expires,
            BoardBuilder::square(10)
                .random(&[
                    ShipDefinition::new("Линкор", 4, 1),
                    ShipDefinition::new("Крейсер", 3, 2),
                    ShipDefinition::new("Эсминец", 2, 3),
                    ShipDefinition::new("Торпеда", 1, 4),
                ])
                .await?,
        )
        .await?;

    cookies.add(
        Cookie::build(("board", id.to_string()))
            .expires(expires)
            .build(),
    );

    Ok(board.lock().await.render().await)
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

    println!("Listening on http://{addr}");

    TcpListener::bind(addr)
        .await
        .context("Failed to bind listener")
}

fn schedule_cleanup(store: StoreAccessor) {
    scheduler::schedule_task(
        "Board data cleanup",
        scheduler::Interval::days(1),
        move || {
            let store = store.clone();
            async move {
                store.write().await.cleanup().await;
            }
        },
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber)?;

    let mut args = Arguments::from_env();
    let listener = listener_from_args(&mut args).await?;

    let store = Arc::new(RwLock::new(store::Store::new()));
    schedule_cleanup(store.clone());

    let router = Router::new()
        .route("/", get(app_handler))
        .route("/game/new", post(new_game_handler))
        .route("/game", post(game_handler))
        .route("/{*path}", get(utils::assets::asset_handler))
        .layer(
            ServiceBuilder::new()
                .layer(CompressionLayer::new())
                .layer(TraceLayer::new_for_http())
                .layer(CookieManagerLayer::new()),
        )
        .with_state(store.clone());

    Ok(axum::serve(listener, router).await.unwrap())
}
