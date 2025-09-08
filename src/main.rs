#![allow(dead_code)]
mod ui;

use std::{cell::RefCell, collections::HashSet, hash::Hash, ops::SubAssign, rc::Rc};

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
    fn try_add_delta(&self, dx: isize, dy: isize) -> Option<Self> {
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
    exposed: bool,
}

impl CellState {
    fn hit(&mut self) -> Result<()> {
        if self.exposed {
            bail!("Cell already hit")
        } else {
            self.expose();
        };

        if let Some(ship) = self.get_ship() {
            ship.borrow_mut().hit();
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
    pub fn hit(&mut self) {
        dbg!("hit");
        match self.length.checked_sub(1) {
            None => return, // Ship already sank
            Some(new_len) => {
                self.length = new_len;
            }
        }

        if self.has_sank() {
            self.sink();
        }
    }

    #[inline]
    pub fn has_sank(&self) -> bool {
        self.length == 0
    }

    fn sink(&mut self) {
        self.counter.borrow_mut().sub_assign(1);

        for cell in &self.nearby_cells {
            cell.borrow_mut().expose();
        }
    }
}

type Vec2D<T> = Vec<Vec<T>>;

#[derive(Shrinkwrap)]
#[shrinkwrap(mutable)]
struct ShipCounter {
    name: String,
    #[shrinkwrap(main_field)]
    remaining: u8,
}

struct Board {
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

    fn hit(&self, point: Point) -> Result<()> {
        self.get_cell(&point)
            .ok_or(anyhow!("Invalid cell coordinates"))?
            .borrow_mut()
            .hit()
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

#[derive(Clone)]
struct ShipDefinition {
    name: String,
    length: u8,
    count: u8,
}

impl ShipDefinition {
    fn to_counter(self) -> ShipCounter {
        ShipCounter {
            name: self.name,
            remaining: self.count,
        }
    }
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
                ship_counters: Vec::new(),
                ships: Vec::new(),
                state,
            },
        }
    }

    fn square(n: u8) -> Self {
        Self::new(Bounds { x: n, y: n })
    }

    fn add_ship_instance(
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

            if cell.borrow_mut().get_ship().is_some() {
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
            counter: counter.clone(),
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

    fn add_ship_manual(&mut self) -> Result<(), ShipAddError> {
        todo!()
    }

    fn add_ship_random(&mut self, mut rng: impl Rng, definition: &ShipDefinition) -> Result<()> {
        self.inner
            .ship_counters
            .push(Rc::new(RefCell::new(definition.clone().to_counter())));

        let counter = self.inner.ship_counters.last().unwrap().clone();

        static TRIES: u16 = 1000;

        for _ in 0..1000 {
            let horizontal = rng.random_bool(0.5);

            let (dx, dy) = if horizontal {
                (definition.length, 1)
            } else {
                (1, definition.length)
            };
            let bounds = Bounds {
                x: self.bounds.x.saturating_sub(dx.into()),
                y: self.bounds.y.saturating_sub(dy.into()),
            };

            let start_x = rng.random_range(0..=bounds.x);
            let start_y = rng.random_range(0..=bounds.y);

            let points: Vec<Point> = (0..definition.length)
                .map(|i| {
                    // Add length according to orientation
                    let (dx, dy) = if horizontal { (i, 0) } else { (0, i) };

                    Point {
                        x: start_x + dx,
                        y: start_y + dy,
                    }
                })
                .collect();

            match self.add_ship_instance(&counter, points) {
                Ok(()) => return Ok(()),
                Err(_) => continue, // Try again with different position
            }
        }
        bail!("Couldn't place a ship after {TRIES} attempts")
    }

    fn random(&mut self, ships: &[ShipDefinition]) -> Result<()> {
        for ship in ships {
            for _ in 0..ship.count {
                self.add_ship_random(rand::rng(), ship)?
            }
        }
        Ok(())
    }

    fn build(self) -> Board {
        self.inner
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
                row_rend.push(if cell.exposed {
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
        name: "test".to_string(),
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
