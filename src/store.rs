use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
};

use anyhow::{Result, anyhow, bail};
use rand::Rng;
use time::{Duration, OffsetDateTime, UtcDateTime};
use tokio::sync::{Mutex, MutexGuard, RwLock};
use tower_cookies::{Cookie, Cookies};

use crate::game::Board;

type SessionID = u16;
// TODO: typed cookies
static SESSION_COOKIE_REF: &str = "board";

pub struct Session {
    expires: OffsetDateTime,
    board: Mutex<Board>,
}

impl Session {
    pub async fn board<'a>(&'a self) -> MutexGuard<'a, Board> {
        self.board.lock().await
    }
}

pub struct Store(HashMap<u16, Session>);

impl Store {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    fn get_vacant_id(&self) -> Option<SessionID> {
        let mut rng = rand::rng();
        let mut id = rng.random::<SessionID>();

        for _ in 0..SessionID::MAX {
            if self.0.contains_key(&id) {
                id = rng.random::<SessionID>();
            } else {
                return Some(id);
            }
        }
        None
    }

    pub fn insert_new<'a>(&'a mut self, session: Session) -> Result<(SessionID, &'a mut Session)> {
        let id = self
            .get_vacant_id()
            .ok_or(anyhow!("Cannot create new session, memory full!"))?;

        let session_ref = match self.0.entry(id) {
            Entry::Occupied(_) => bail!("get_vacant_id returned occupied id"),
            Entry::Vacant(entry) => entry.insert(session),
        };

        Ok((id, session_ref))
    }

    pub fn new_session<'a>(
        &'a mut self,
        cookies: &Cookies,
        board: Board,
    ) -> Result<&'a mut Session> {
        let now = OffsetDateTime::now_utc();
        let expires = now + Duration::days(1);

        let (id, session) = self.insert_new(Session {
            expires,
            board: Mutex::new(board),
        })?;

        cookies.add(
            Cookie::build((SESSION_COOKIE_REF, id.to_string()))
                .expires(expires)
                .build(),
        );

        tracing::info!("New board created: {}", id);
        Ok(session)
    }

    // TODO: do not expose tuple
    pub fn get_session(&self, cookies: &Cookies) -> Option<(SessionID, &Session)> {
        // TODO: maybe propagate parse error
        let id = cookies.get(SESSION_COOKIE_REF)?.value().parse().ok()?;
        Some((id, self.0.get(&id)?))
    }

    pub fn remove_session(&mut self, id: SessionID, cookies: &Cookies) {
        cookies.remove(SESSION_COOKIE_REF.into());
        self.0.remove(&id);
    }

    pub async fn cleanup(&mut self) {
        let now = UtcDateTime::now();
        let _ = self.0.extract_if(|_, entry| entry.expires < now);
        tracing::info!("Cleaned up board data")
    }
}

pub type StoreAccessor = Arc<RwLock<Store>>;
