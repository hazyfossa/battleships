use maud::{Markup, html};

use crate::game::{Board, CellState, Point, ShipCounter};

fn int_to_letter(value: usize) -> char {
    // NOTE: Ё :(
    const ALPHABET: &str = "АБВГДЕЖЗИКЛМНОПРСТУФХЦЧШЩЭЮЯ";
    ALPHABET.chars().nth(value).unwrap_or('~')
}

impl Board {
    pub async fn render(&self) -> Markup {
        html! {
            #screen {
            #display .game {
                #stats-container {
                    @for counter in &self.ship_counters {
                        @let counter = counter.read().await;
                        @if !counter.is_defeated() {
                            (counter.render())
                        }
                    }
                }

                #board {
                    style {
                        (format!(
                            "#board {{ grid-template-columns: repeat({}, 1fr) }}",
                            self.state.len() + 1
                        ))
                    }

                    div .cell .ui { };
                    @for i in (0..self.state.len()) {
                        div .cell .ui {(int_to_letter(i))}
                    }

                    @for (x, row) in self.state.iter().enumerate() {
                        div .cell .ui {(x+1)}
                        @for (y, cell) in row.iter().enumerate() {
                            (cell.read().await.render(x, y))
                        }
                    }
                }
            }}
        }
    }
}

pub fn render_win() -> Markup {
    html!({
        #screen .waves {
        #display .waves {
            #win-card {"Победа!"}
        }}
    })
}

impl CellState {
    fn render(&self, x: usize, y: usize) -> Markup {
        let point = Point::from_index(x, y);
        html!({
            @if self.exposed {
                div id=(point) class={@if self.contains_ship() {"cell ship"} @else {"cell water"}} {}
            } @else {
                div id=(point) class="cell active" hx-patch={"game?hit="(point)} hx-target="body" {}
            }
        })
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
