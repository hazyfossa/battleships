#![allow(dead_code)]
mod utils;

use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt::Display,
    hash::Hash,
    ops::SubAssign,
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
use axum::{
    Router, extract,
    response::IntoResponse,
    routing::{get, post},
};
use maud::{Markup, html};
use pico_args::Arguments;
use rand::Rng;
use serde::Deserialize;
use shrinkwraprs::Shrinkwrap;
use tokio::{
    net::TcpListener,
    sync::{Mutex, MutexGuard, RwLock},
};
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;

use crate::utils::errors::Fallible;

type Dyn<T> = Arc<RwLock<T>>;

// TODO: I suspect x/y cooors on board are rotated and what we call row is actually a column

type BoardID = u16;

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct Point {
    x: u8,
    y: u8,
}

impl Point {
    fn new(x: u8, y: u8) -> Self {
        Self { x, y }
    }

    fn from_index(x: usize, y: usize) -> Self {
        Self::new(x as u8, y as u8)
    }

    fn try_add_delta(&self, dx: isize, dy: isize) -> Option<Self> {
        Some(Point {
            x: (self.x as isize + dx).try_into().ok()?,
            y: (self.y as isize + dy).try_into().ok()?,
        })
    }
}

impl Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.x, self.y)
    }
}

impl<'de> Deserialize<'de> for Point {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        let (x, y) = value
            .split_once("-")
            .ok_or(serde::de::Error::custom(format!(
                "expected format 'x-y', got '{value}'",
            )))?;

        Ok(Self::new(
            x.parse().map_err(serde::de::Error::custom)?,
            y.parse().map_err(serde::de::Error::custom)?,
        ))
    }
}

type Bounds = Point; // Bounds are just the maximum point in both coordinates

enum CellContent {
    Water,
    NearShip(Dyn<Ship>),
    Ship(Dyn<Ship>),
}

impl CellContent {
    fn contains_ship(&self) -> bool {
        match self {
            Self::Ship(_) => true,
            _ => false,
        }
    }

    fn get_ship(&mut self) -> Option<Dyn<Ship>> {
        match self {
            Self::Ship(ship) => Some(ship.clone()),
            _ => None,
        }
    }

    fn get_collision(&self) -> Option<Dyn<Ship>> {
        match self {
            Self::Ship(ship) => Some(ship.clone()),
            Self::NearShip(ship) => Some(ship.clone()),
            _ => None,
        }
    }
}

#[derive(Shrinkwrap)]
#[shrinkwrap(mutable)]
struct CellState {
    #[shrinkwrap(main_field)]
    content: CellContent,
    exposed: bool,
}

impl CellState {
    async fn hit(&mut self) -> Result<()> {
        if self.exposed {
            bail!("Cell already hit")
        } else {
            self.expose();
        };

        if let Some(ship) = self.get_ship() {
            ship.write().await.hit().await;
        }

        Ok(())
    }

    #[inline]
    fn expose(&mut self) {
        self.exposed = true;
    }
}

impl Default for CellState {
    fn default() -> Self {
        Self {
            content: CellContent::Water,
            exposed: false,
        }
    }
}

struct Ship {
    length: u8,
    nearby_cells: Vec<Dyn<CellState>>,
    counter: Dyn<ShipCounter>,
}

impl Ship {
    async fn hit(&mut self) {
        match self.length.checked_sub(1) {
            None => return, // Ship already sank
            Some(new_len) => {
                self.length = new_len;
            }
        }

        if self.has_sank() {
            self.sink().await;
        }
    }

    #[inline]
    fn has_sank(&self) -> bool {
        self.length == 0
    }

    async fn sink(&mut self) {
        self.counter.write().await.sub_assign(1);

        for cell in &self.nearby_cells {
            cell.write().await.expose();
        }
    }
}

type Vec2D<T> = Vec<Vec<T>>;

#[derive(Shrinkwrap)]
#[shrinkwrap(mutable)]
struct ShipCounter {
    name: String,
    total: u8,
    #[shrinkwrap(main_field)]
    remaining: u8,
}

impl ShipCounter {
    fn new(name: String, n: u8) -> Self {
        Self {
            name,
            total: n,
            remaining: n,
        }
    }

    fn is_defeated(&self) -> bool {
        self.remaining == 0
    }
}

struct BoardData {
    ships: Vec<Dyn<Ship>>,
    ship_counters: Vec<Dyn<ShipCounter>>,
    state: Vec2D<Dyn<CellState>>,
}

impl BoardData {
    fn get_cell(&self, point: &Point) -> Option<Dyn<CellState>> {
        self.state
            .get(point.x as usize)?
            .get(point.y as usize)
            .cloned()
    }

    async fn hit(&self, point: Point) -> Result<()> {
        self.get_cell(&point)
            .ok_or(anyhow!("Invalid cell coordinates"))?
            .write()
            .await
            .hit()
            .await
    }

    async fn is_win(&self) -> bool {
        // TODO: if we can do counters without RwLock,
        // this can be a much cleaner .iter().map(...).all()

        let mut win = true;
        for counter in &self.ship_counters {
            let defeated = counter.read().await.is_defeated();
            if !defeated {
                win = false;
                break;
            }
        }
        win
    }
}

struct BoardBuilder {
    bounds: Point,
    inner: BoardData,
}

enum ShipAddError {
    Collision { point: Point },
    OutOfBounds,
    InternalError(anyhow::Error),
}

impl From<&str> for ShipAddError {
    fn from(value: &str) -> Self {
        Self::InternalError(anyhow!(value.to_string()))
    }
}

#[derive(Clone)]
struct ShipDefinition {
    name: String,
    length: u8,
    count: u8,
}

impl ShipDefinition {
    fn new(name: &str, length: u8, count: u8) -> Self {
        Self {
            name: name.to_string(),
            length,
            count,
        }
    }

    fn to_counter(self) -> ShipCounter {
        ShipCounter::new(self.name, self.count)
    }
}

impl BoardBuilder {
    fn new(bounds: Bounds) -> Self {
        let state = (0..bounds.x)
            .map(|_| {
                (0..bounds.y)
                    .map(|_| Arc::new(RwLock::new(CellState::default())))
                    .collect()
            })
            .collect();

        Self {
            bounds,
            inner: BoardData {
                ship_counters: Vec::new(),
                ships: Vec::new(),
                state,
            },
        }
    }

    fn square(n: u8) -> Self {
        Self::new(Bounds { x: n, y: n })
    }

    async fn add_ship_instance(
        &mut self,
        counter: &Dyn<ShipCounter>,
        points: Vec<Point>,
    ) -> Result<(), ShipAddError> {
        if points.is_empty() {
            return Err("Ship requires at least one point".into());
        };

        let mut ship_cells = Vec::new();
        let mut near_cells = Vec::new();

        for &point in &points {
            let cell = self
                .inner
                .get_cell(&point)
                .ok_or(ShipAddError::OutOfBounds)?;

            if cell.read().await.contains_ship() {
                return Err(ShipAddError::Collision { point }); // TODO: maybe return ship here
            }

            let mut tried_points = HashSet::new();

            // Collect adjacent points (including diagonals) for collision checking
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(adjacent_point) = point.try_add_delta(dx, dy) {
                        // Only add if it's not part of the ship itself,
                        // and we haven't reached the same point via delta from another cell
                        if !points.contains(&adjacent_point)
                            && !tried_points.contains(&adjacent_point)
                        {
                            tried_points.insert(adjacent_point);

                            if let Some(cell) = self.inner.get_cell(&adjacent_point) {
                                if cell.read().await.contains_ship() {
                                    return Err(ShipAddError::Collision {
                                        point: adjacent_point,
                                    });
                                }
                                near_cells.push(cell);
                            }
                        }
                    }
                }
            }
            ship_cells.push(cell);
        }

        // No collisions detected, proceed with placing the ship
        self.inner.ships.push(Arc::new(RwLock::new(Ship {
            length: points.len() as u8,
            nearby_cells: near_cells.clone(),
            counter: counter.clone(),
        })));
        let ship = self.inner.ships.last().unwrap(); // TODO: is this always safe

        for cell in ship_cells {
            cell.write().await.content = CellContent::Ship(ship.clone())
        }

        for cell in near_cells {
            cell.write().await.content = CellContent::NearShip(ship.clone())
        }

        Ok(())
    }

    fn add_ship_manual(&mut self) -> Result<(), ShipAddError> {
        todo!()
    }

    async fn add_ship_random(&mut self, length: u8, counter: &Dyn<ShipCounter>) -> Result<()> {
        static TRIES: u16 = 1000;

        // TODO: less rng cell bindings

        for _ in 0..1000 {
            let horizontal = rand::rng().random_bool(0.5);

            let (dx, dy) = if horizontal { (length, 1) } else { (1, length) };
            let bounds = Bounds {
                x: self.bounds.x.saturating_sub(dx.into()),
                y: self.bounds.y.saturating_sub(dy.into()),
            };

            let start_x = rand::rng().random_range(0..=bounds.x);
            let start_y = rand::rng().random_range(0..=bounds.y);

            let points: Vec<Point> = (0..length)
                .map(|i| {
                    // Add length according to orientation
                    let (dx, dy) = if horizontal { (i, 0) } else { (0, i) };

                    Point {
                        x: start_x + dx,
                        y: start_y + dy,
                    }
                })
                .collect();

            match self.add_ship_instance(&counter, points).await {
                Ok(()) => {
                    return Ok(());
                }
                Err(_) => continue, // Try again with different position
            }
        }
        bail!("Couldn't place a ship after {TRIES} attempts")
    }

    async fn random(mut self, ships: &[ShipDefinition]) -> Result<BoardData> {
        for ship in ships {
            self.inner
                .ship_counters
                .push(Arc::new(RwLock::new(ship.clone().to_counter())));

            let counter = self.inner.ship_counters.last().unwrap().clone();

            for _ in 0..ship.count {
                self.add_ship_random(ship.length, &counter).await?
            }
        }
        Ok(self.inner)
    }
}

// impl Board {
//     fn cli_render(&self) {
//         for row in self.state.clone() {
//             let mut row_rend = Vec::new();

//             for cell in row {
//                 let cell = cell.borrow();
//                 let cell_rend = match cell.content {
//                     CellContent::Water => "W",
//                     CellContent::NearShip(_) => "N",
//                     CellContent::Ship(_) => "S",
//                 };
//                 row_rend.push(if cell.exposed {
//                     "(".to_owned() + cell_rend + ")"
//                 } else {
//                     "[-]".to_owned()
//                 })
//             }

//             println!("{}", row_rend.join(" "))
//         }
//     }
// }

#[derive(Shrinkwrap)]
struct Board<'a> {
    id: u16,
    #[shrinkwrap(main_field)]
    data: MutexGuard<'a, BoardData>,
}

impl Board<'_> {
    async fn render(&self) -> Markup {
        if self.is_win().await {
            render_win()
        } else {
            html! {
                #screen {
                    #stats-container {
                        @for counter in &self.ship_counters {
                            @let counter = counter.read().await;
                            @if !counter.is_defeated() {
                                (counter.render())
                            }
                        }
                    }

                    table #board {
                    tbody {
                        @for (x, row) in self.state.iter().enumerate() {
                                tr {
                                @for (y, cell) in row.iter().enumerate() {
                                    (cell.read().await.render(self.id, x, y))
                                }
                            }
                        }
                    }}
                }
            }
        }
    }
}

// TODO: anchor

impl CellState {
    fn render(&self, id: BoardID, x: usize, y: usize) -> Markup {
        let point = Point::from_index(x, y);
        html!({
            @if self.exposed {
                td id=(point) class={@if self.contains_ship() {"ship"} @else {"water"}};
            } @else {
                td .active-cell
                hx-post={"render?board="(id)"&point="(point)}
                hx-target="#container";
            }
        }
        )
    }
}

impl ShipCounter {
    fn render(&self) -> Markup {
        html!(.ship-counter {
            .cnt-name {(self.name)}
            .cnt-row {
                .cnt-remaining {(self.remaining)} "/" .cnt-total {(self.total)}
            }
        })
    }
}

fn render_win() -> Markup {
    html!({
        #win-card {"Победа!"}
    })
}

struct Store(HashMap<u16, Mutex<BoardData>>);

impl Store {
    fn new() -> Self {
        Self(HashMap::new())
    }

    async fn new_board<'a>(&'a mut self, data: BoardData) -> Result<Board<'a>> {
        let id = (|| {
            let mut rng = rand::rng();
            let mut id = rng.random::<BoardID>();
            for _ in 0..BoardID::MAX {
                if self.0.contains_key(&id) {
                    id = rng.random::<BoardID>();
                    continue;
                }
            }
            id
        })();

        let data_ref = match self.0.entry(id) {
            Entry::Occupied(_) => bail!("Cannot create new board, memory full!"),
            Entry::Vacant(entry) => entry.insert(Mutex::new(data)),
        };

        Ok(Board {
            id,
            data: data_ref.lock().await,
        })
    }

    async fn get_board<'a>(&'a self, id: BoardID) -> Option<Board<'a>> {
        Some(Board {
            id,
            data: self.0.get(&id)?.lock().await,
        })
    }
}

#[derive(Deserialize)]
struct RenderRequestData {
    board: BoardID,
    point: Point,
}

#[axum::debug_handler]
async fn board_handler(
    store: extract::State<Arc<Mutex<Store>>>,
    extract::Query(data): extract::Query<RenderRequestData>,
) -> Fallible<impl IntoResponse> {
    let store = store.lock().await;
    let board = match store.get_board(data.board).await {
        Some(board) => board,
        None => return Err(anyhow!("Board not found").into()),
    };

    board.hit(data.point).await?;

    Ok(board.render().await)
}

#[axum::debug_handler]
async fn new_board_handler(
    store: extract::State<Arc<Mutex<Store>>>,
) -> Fallible<impl IntoResponse> {
    let mut store = store.lock().await;

    let board = store
        .new_board(
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
                            hx-post={"/render/new"}
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

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = Arguments::from_env();
    let listener = listener_from_args(&mut args).await?;

    let store = Arc::new(Mutex::new(Store::new()));

    let router = Router::new()
        .route("/", get(app_handler))
        .route("/render/new", post(new_board_handler))
        .route("/render", post(board_handler))
        .route("/{*path}", get(utils::assets::asset_handler))
        .layer(ServiceBuilder::new().layer(CompressionLayer::new()))
        .with_state(store);

    Ok(axum::serve(listener, router).await.unwrap())
}
