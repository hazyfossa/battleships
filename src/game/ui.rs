use maud::{Markup, PreEscaped, html};

use crate::game::{Board, CellRef, CellState, HitDisplayDiff, Point, ShipCounter};

// TODO: some stuff can be much better if we replace maud with a typed html engine that understands htmx
// Unfortunately, no such thing exists from my knowledge

fn int_to_letter(value: usize) -> char {
    // NOTE: Ё :(
    const ALPHABET: &str = "АБВГДЕЖЗИКЛМНОПРСТУФХЦЧШЩЭЮЯ";
    ALPHABET.chars().nth(value).unwrap_or('~')
}

enum RenderMode {
    Paint,
    Replace,
}

impl RenderMode {
    fn element(&self, id: String, class: &'static str, html: Markup) -> Markup {
        html!({
            @if matches!(self, Self::Replace) {
                div id=(id) class=(PreEscaped(class)) hx-swap-oob="true" {(html)}
            } @else {
                div id=(id) class=(PreEscaped(class)) {(html)}
            }
        })
    }
}

impl Board {
    pub async fn render(&self) -> Markup {
        html! {
            #screen {
            #display .game {
                #stats-container {
                    @for counter in &self.ship_counters {
                        (counter.read().await.render(RenderMode::Paint))
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
                            @let point = Point::from_index(x,y);
                            (cell.read().await.render(point, RenderMode::Paint))
                        }
                    }
                }
            }}
        }
    }
}

impl ShipCounter {
    fn render(&self, mode: RenderMode) -> Markup {
        let class = match self.is_defeated() {
            true => "ship-counter defeated",
            false => "ship-counter",
        };

        mode.element(
            self.name.clone(), // TODO: id independent of ship name
            class,
            html!({
                .cnt-name {(self.name)}
                .cnt-row {
                    .cnt-remaining {(self.remaining)} "/" .cnt-total {(self.total)}
                }
            }),
        )
    }
}

impl CellState {
    fn render(&self, point: Point, mode: RenderMode) -> Markup {
        if self.exposed {
            let class = match self.contains_ship() {
                true => "cell ship",
                false => "cell water",
            };

            mode.element(point.to_string(), class, PreEscaped("".into()))
        } else {
            html!({
                div id=(point) class="cell active" hx-patch="game" {}
            })
        }
    }
}

impl CellRef {
    async fn render(&self, mode: RenderMode) -> Markup {
        self.accessor.read().await.render(self.point, mode)
    }
}

impl HitDisplayDiff {
    pub async fn render(&self) -> Markup {
        let mut result = self.cell.render(RenderMode::Paint).await.into_string();

        if let Some(cells) = &self.additional_cells {
            for cell in cells {
                let rendered = cell.render(RenderMode::Replace).await.into_string();
                result.push_str(&rendered);
            }
        };

        if let Some(counters) = &self.refresh_counters {
            for counter in counters {
                let rendered = counter
                    .read()
                    .await
                    .render(RenderMode::Replace)
                    .into_string();

                result.push_str(&rendered);
            }
        }

        PreEscaped(result)
    }
}
