#![allow(dead_code)] // TODO
mod ui;

use anyhow::{Result, anyhow, bail};
use rand::Rng;
use serde::Deserialize;
use shrinkwraprs::Shrinkwrap;
use tokio::sync::RwLock;

use std::{collections::HashSet, fmt::Display, hash::Hash, ops::SubAssign, sync::Arc};

use crate::Dyn;

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub struct Point {
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

pub struct Board {
    ships: Vec<Dyn<Ship>>,
    ship_counters: Vec<Dyn<ShipCounter>>,
    state: Vec2D<Dyn<CellState>>,
}

impl Board {
    fn get_cell(&self, point: &Point) -> Option<Dyn<CellState>> {
        self.state
            .get(point.x as usize)?
            .get(point.y as usize)
            .cloned()
    }

    pub async fn hit(&self, point: Point) -> Result<()> {
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
pub struct ShipDefinition {
    name: String,
    length: u8,
    count: u8,
}

impl ShipDefinition {
    pub fn new(name: &str, length: u8, count: u8) -> Self {
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

pub struct BoardBuilder {
    bounds: Point,
    inner: Board,
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
            inner: Board {
                ship_counters: Vec::new(),
                ships: Vec::new(),
                state,
            },
        }
    }

    pub fn square(n: u8) -> Self {
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

            if cell.read().await.get_collision().is_some() {
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
                                // TODO: is this check redundant
                                // considering we checked for collisions above?
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

    pub async fn random(mut self, ships: &[ShipDefinition]) -> Result<Board> {
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
