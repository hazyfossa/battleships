#![allow(dead_code)]
mod ui;

use std::{cell::RefCell, fmt::Write, rc::Rc};

use anyhow::{Result, anyhow, bail};
use maud::{Render, html};
use rand::Rng;
use shrinkwraprs::Shrinkwrap;

// TODO: this ideally shouldn't exist
type Dyn<T> = Rc<RefCell<T>>;

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct Point {
    x: u8,
    y: u8,
}

impl Point {
    fn clone_with_delta(&self, dx: isize, dy: isize) -> Option<Self> {
        Some(Point {
            x: (self.x as isize + dx).try_into().ok()?,
            y: (self.y as isize + dy).try_into().ok()?,
        })
    }
}

type Bounds = Point; // Bounds are just the maximum point in both coordinates

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

struct ShipDefinition {
    length: u8,
    count: u8,
}

impl BoardBuilder {
    fn new(bounds: Bounds) -> Self {
        let state = (0..=bounds.x)
            .map(|_| {
                (0..=bounds.y)
                    .map(|_| Rc::new(RefCell::new(CellState::default())))
                    .collect()
            })
            .collect();

        Self {
            bounds,
            inner: Board {
                ships: Vec::new(),
                state,
            },
        }
    }

    fn square(n: u8) -> Self {
        Self::new(Bounds { x: n, y: n })
    }

    fn add_ship(&mut self, points: Vec<Point>) -> Result<(), ShipAddError> {
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

    fn add_ship_random(&mut self, mut rng: impl Rng, length: u8) -> Result<()> {
        static TRIES: u16 = 1000;

        for _ in 0..1000 {
            let horizontal = rng.random_bool(0.5);

            let (dx, dy) = if horizontal { (length, 1) } else { (1, length) };
            let bounds = Bounds {
                x: self.bounds.x.saturating_sub(dx.into()),
                y: self.bounds.y.saturating_sub(dy.into()),
            };

            let start_x = rng.random_range(0..=bounds.x);
            let start_y = rng.random_range(0..=bounds.y);

            // Generate ship points
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

            match self.add_ship(points) {
                Ok(()) => return Ok(()),
                Err(_) => continue, // Try again with different position
            }
        }
        bail!("Couldn't place a ship after {TRIES} attempts")
    }

    fn random(&mut self, ship_defs: &[ShipDefinition]) -> Result<()> {
        for ship_def in ship_defs {
            for _ in 0..ship_def.count {
                self.add_ship_random(rand::rng(), ship_def.length)?
            }
        }
        Ok(())
    }

    fn build(self) -> Board {
        self.inner
    }
}

type Vec2D<T> = Vec<Vec<T>>;

struct Board {
    ships: Vec<Dyn<Ship>>,
    state: Vec2D<Dyn<CellState>>,
}

impl Board {
    fn get_cell(&self, point: &Point) -> Option<Dyn<CellState>> {
        self.state
            .get(point.x as usize)?
            .get(point.y as usize)
            .cloned()
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

// impl Render for Board {
//     fn render(&self) -> maud::Markup {
//         for row in self.state {
//             for cell in row {
//                 match cell
//             }
//         }
//     }
// }

impl Board {
    fn cli_render(&self) {
        for row in self.state.clone() {
            let mut row_rend = Vec::new();

            for cell in row {
                let cell = cell.borrow();
                let cell_rend = match cell.content {
                    CellContent::Water => "W",
                    CellContent::NearShip(_) => "N",
                    CellContent::Ship(_) => "S",
                };
                row_rend.push(if cell.hit {
                    "(".to_owned() + cell_rend + ")"
                } else {
                    "[-]".to_owned()
                })
            }

            println!("{}", row_rend.join(" "))
        }
    }
}

fn main() -> Result<()> {
    let mut board = BoardBuilder::square(10);
    board.random(&[ShipDefinition {
        length: 2,
        count: 5,
    }])?;

    let board = board.build();

    board.cli_render();
    println!("\n\n");

    for x in 1..5 {
        for y in 1..5 {
            board.hit(Point { x, y });
        }
    }

    board.cli_render();

    Ok(())
}
