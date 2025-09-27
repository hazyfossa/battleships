use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
};

use anyhow::{Result, bail};
use rand::Rng;
use shrinkwraprs::Shrinkwrap;
use time::{OffsetDateTime, UtcDateTime};
use tokio::sync::{Mutex, RwLock};
use tracing::{Level, event};

use crate::game::Board;

type BoardID = u16;

#[derive(Shrinkwrap)]
pub struct StoredBoard {
    expires: OffsetDateTime,
    #[shrinkwrap(main_field)]
    inner: Mutex<Board>,
}

pub struct Store(HashMap<u16, StoredBoard>);

impl Store {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub async fn new_board<'a>(
        &'a mut self,
        expires: OffsetDateTime,
        data: Board,
    ) -> Result<(BoardID, &'a mut Mutex<Board>)> {
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

        let board_ref = match self.0.entry(id) {
            Entry::Occupied(_) => bail!("Cannot create new board, memory full!"),
            Entry::Vacant(entry) => entry.insert(StoredBoard {
                expires,
                inner: Mutex::new(data),
            }),
        };

        event!(Level::INFO, "New board created: {}", id);
        Ok((id, &mut board_ref.inner))
    }

    pub async fn get_board<'a>(&'a self, id: BoardID) -> Option<&'a StoredBoard> {
        self.0.get(&id)
    }

    pub async fn cleanup(&mut self) {
        let now = UtcDateTime::now();
        let _ = self.0.extract_if(|_, entry| entry.expires < now);
        event!(Level::INFO, "Cleaned up board data")
    }
}

pub type StoreAccessor = Arc<RwLock<Store>>;
