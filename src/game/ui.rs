use maud::{Markup, html};

use crate::game::{Board, CellState, Point, ShipCounter};

fn int_to_letter(value: usize) -> char {
    // NOTE: Ё :(
    const ALPHABET: &str = "АБВГДЕЖЗИКЛМНОПРСТУФХЦЧШЩЭЮЯ";
    ALPHABET.chars().nth(value).unwrap_or('~')
}

impl Board {
    pub async fn render(&self) -> Markup {
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
                        tr { // Header
                            th .cell {};
                            @for i in (0..self.state.len()) {
                                th .cell {(int_to_letter(i))};
                            }
                        }
                        @for (x, row) in self.state.iter().enumerate() {
                                tr {
                                th .cell {(x+1)};
                                @for (y, cell) in row.iter().enumerate() {
                                    (cell.read().await.render(x, y))
                                }
                            }
                        }
                    }}
                }
            }
        }
    }
}

fn render_win() -> Markup {
    html!({
        #screen .waves {
            #win-card {"Победа!"}
        }
    })
}

impl CellState {
    fn render(&self, x: usize, y: usize) -> Markup {
        let point = Point::from_index(x, y);
        html!({
            @if self.exposed {
                td id=(point) class={@if self.contains_ship() {"cell ship"} @else {"cell water"}};
            } @else {
                td id=(point) .cell .active
                hx-post={"game?hit="(point)}
                hx-target="#container";
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
