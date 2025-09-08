#![allow(dead_code)]
mod ui;

use std::{cell::RefCell, rc::Rc};

use anyhow::{Result, anyhow, bail};
use maud::{Render, html};
use shrinkwraprs::Shrinkwrap;

type Dyn<T> = Rc<RefCell<T>>;

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct Point {
    x: usize,
    y: usize,
}

impl Point {
    fn clone_with_delta(&self, dx: isize, dy: isize) -> Option<Self> {
        Some(Point {
            x: (self.x as isize + dx).try_into().ok()?,
            y: (self.y as isize + dy).try_into().ok()?,
        })
    }
}

enum CellContent {
    Water,
    NearShip(Dyn<Ship>),
    Ship(Dyn<Ship>),
}

impl CellContent {
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
    hit: bool,
}

impl CellState {
    fn hit(&mut self) -> Result<()> {
        if self.hit {
            bail!("Cell already hit")
        } else {
            self.hit = true
        };
        Ok(())
    }

    fn hit_if_not_already(&mut self) {
        self.hit = true
    }
}

impl Default for CellState {
    fn default() -> Self {
        Self {
            content: CellContent::Water,
            hit: false,
        }
    }
}

struct Ship {
    length: u8,
    nearby_cells: Vec<Dyn<CellState>>,
}

impl Ship {
    fn hit(&mut self) -> bool {
        let new_len = self.length.checked_sub(1);

        match new_len {
            None => true,
            Some(x) => {
                self.length = x;
                x == 0
            }
        }
    }
}

struct BoardBuilder {
    bounds: Point,
    inner: Board,
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

impl BoardBuilder {
    fn new(bounds: Point) -> Self {
        #[rustfmt::skip]
        let state = (0..=bounds.x)
            .map(|_| (0..=bounds.y)
                .map(|_| Rc::new(RefCell::new(CellState::default())))
            .collect())
        .collect();

        Self {
            bounds,
            inner: Board {
                ships: Vec::new(),
                state,
            },
        }
    }

    pub fn add_ship(&mut self, points: Vec<Point>) -> Result<(), ShipAddError> {
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

            if cell.borrow_mut().get_ship().is_some() {
                return Err(ShipAddError::Collision { point }); // TODO: maybe return ship here
            }

            // Collect adjacent points (including diagonals) for collision checking
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(adjacent_point) = point.clone_with_delta(dx, dy) {
                        // Only add if it's not part of the ship itself
                        if !points.contains(&adjacent_point) {
                            if let Some(cell) = self.inner.get_cell(&adjacent_point) {
                                if cell.borrow_mut().get_ship().is_some() {
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
        self.inner.ships.push(Rc::new(RefCell::new(Ship {
            length: points.len() as u8,
            nearby_cells: near_cells.clone(),
        })));
        let ship = self.inner.ships.last().unwrap(); // TODO: is this always safe

        for cell in ship_cells {
            cell.borrow_mut().content = CellContent::Ship(ship.clone())
        }

        for cell in near_cells {
            cell.borrow_mut().content = CellContent::NearShip(ship.clone())
        }

        Ok(())
    }
}

type Vec2D<T> = Vec<Vec<T>>;

pub struct Board {
    ships: Vec<Dyn<Ship>>,
    state: Vec2D<Dyn<CellState>>,
}

impl Board {
    fn get_cell(&self, point: &Point) -> Option<Dyn<CellState>> {
        self.state.get(point.x)?.get(point.y).cloned()
    }

    fn hit(&self, point: Point) -> Result<()> {
        let cell = self
            .get_cell(&point)
            .ok_or(anyhow!("Invalid cell coordinates"))?;

        cell.borrow_mut().hit()?;

        Ok(match cell.borrow_mut().get_ship() {
            None => (),
            Some(ship) => {
                let mut ship = ship.borrow_mut();
                let has_sank = ship.hit();
                if has_sank {
                    for cell in &ship.nearby_cells {
                        cell.borrow_mut().hit_if_not_already();
                    }
                }
            }
        })
    }
}

impl Render for Board {
    fn render(&self) -> maud::Markup {
        html! {
            table ;
        }
    }
}

fn main() -> Result<()> {
    todo!()
}
