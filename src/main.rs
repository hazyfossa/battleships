#![allow(dead_code)]
mod ui;

use anyhow::{Result, anyhow, bail};
use maud::{Render, html};
use shrinkwraprs::Shrinkwrap;

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

enum CellContent<'a> {
    Water,
    NearShip(&'a mut Ship<'a>),
    Ship(&'a mut Ship<'a>),
}

impl<'a> CellContent<'a> {
    fn get_ship(&mut self) -> Option<&'a mut Ship> {
        match self {
            Self::Ship(ship) => Some(ship),
            _ => None,
        }
    }

    fn get_collision(&self) -> Option<&'a Ship> {
        match self {
            Self::Ship(ship) => Some(ship),
            Self::NearShip(ship) => Some(ship),
            _ => None,
        }
    }
}

#[derive(Shrinkwrap)]
#[shrinkwrap(mutable)]
struct CellState<'a> {
    #[shrinkwrap(main_field)]
    content: CellContent<'a>,
    hit: bool,
}

impl Default for CellState<'_> {
    fn default() -> Self {
        Self {
            content: CellContent::Water,
            hit: false,
        }
    }
}

struct Ship<'a> {
    length: u8,
    nearby_cells: Vec<&'a mut CellState<'a>>,
}

impl<'a> Ship<'a> {
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

struct BoardBuilder<'a> {
    bounds: Point,
    inner: Board<'a>,
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

impl BoardBuilder<'_> {
    fn new(bounds: Point) -> Self {
        #[rustfmt::skip]
        let state = (0..=bounds.x)
            .map(|_| (0..=bounds.y)
                .map(|_| CellState::default())
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

            if cell.get_ship().is_some() {
                return Err(ShipAddError::Collision { point }); // TODO: maybe return ship here
            }

            // Collect adjacent points (including diagonals) for collision checking
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(adjacent_point) = point.clone_with_delta(dx, dy) {
                        // Only add if it's not part of the ship itself
                        if !points.contains(&adjacent_point) {
                            if let Some(ref cell) = self.inner.get_cell(&adjacent_point) {
                                if cell.get_ship().is_some() {
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
        self.inner.ships.push(Ship {
            length: points.len() as u8,
            nearby_cells: near_cells,
        });
        let ship = self.inner.ships.last_mut().unwrap(); // TODO: is this always safe

        for cell in ship_cells {
            cell.content = CellContent::Ship(ship)
        }

        for cell in near_cells {
            cell.content = CellContent::NearShip(ship)
        }

        Ok(())
    }
}

type Vec2D<T> = Vec<Vec<T>>;

pub struct Board<'board> {
    ships: Vec<Ship<'board>>,
    state: Vec2D<CellState<'board>>,
}

impl<'a> Board<'a> {
    fn get_cell(&mut self, point: &Point) -> Option<&'a mut CellState> {
        self.state.get_mut(point.x)?.get_mut(point.y)
    }

    pub fn hit(&'a mut self, point: Point) -> Result<()> {
        let cell = self
            .get_cell(&point)
            .ok_or(anyhow!("Invalid cell coordinates"))?;

        if cell.hit {
            bail!("Cell already hit")
        } else {
            cell.hit = true
        };

        Ok(match cell.get_ship() {
            None => (),
            Some(ref mut ship) => {
                let has_sank = ship.hit();
                if has_sank {
                    for cell in &mut ship.nearby_cells {
                        cell.hit = true;
                    }
                }
            }
        })
    }
}

impl Render for Board<'_> {
    fn render(&self) -> maud::Markup {
        html! {
            table ;
        }
    }
}

fn main() -> Result<()> {
    todo!()
}
