use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use dashmap::{
    DashMap, Entry,
    mapref::one::{Ref, RefMut},
};
use rand::Rng;
use time::{Duration, OffsetDateTime, UtcDateTime};
use tower_cookies::{Cookie, Cookies};

use crate::game::Board;

type SessionID = u16;
// TODO: typed cookies
static SESSION_COOKIE_REF: &str = "board";

pub struct Session {
    expires: OffsetDateTime,
    pub board: Board,
}

type SessionRef<'a> = Ref<'a, SessionID, Session>;
type SessionRefMut<'a> = RefMut<'a, SessionID, Session>;

pub struct Store(DashMap<u16, Session>);

impl<'a> Store {
    pub fn new() -> Self {
        Self(DashMap::new())
    }

    fn get_vacant_id(&self) -> Option<SessionID> {
        // TODO: optimize
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

    pub fn insert_new(&'a self, session: Session) -> Result<SessionRefMut<'a>> {
        let id = self
            .get_vacant_id()
            .ok_or(anyhow!("Cannot create new session, memory full!"))?;

        let session_ref = match self.0.entry(id) {
            Entry::Occupied(_) => bail!("get_vacant_id returned occupied id"),
            Entry::Vacant(entry) => entry.insert(session),
        };

        Ok(session_ref)
    }

    pub fn new_session(&'a self, cookies: &Cookies, board: Board) -> Result<SessionRefMut<'a>> {
        let now = OffsetDateTime::now_utc();
        let expires = now + Duration::days(1);

        let session = self.insert_new(Session { expires, board })?;

        let id = session.key();

        cookies.add(
            Cookie::build((SESSION_COOKIE_REF, id.to_string()))
                .expires(expires)
                .build(),
        );

        tracing::info!("New board created: {}", id);
        Ok(session)
    }

    pub fn get_session(&'a self, cookies: &Cookies) -> Option<SessionRef<'a>> {
        // TODO: maybe propagate parse error
        let id = cookies.get(SESSION_COOKIE_REF)?.value().parse().ok()?;
        Some(self.0.get(&id)?)
    }

    pub async fn remove_session(&self, session: SessionRef<'a>, cookies: &Cookies) {
        cookies.remove(SESSION_COOKIE_REF.into());

        let id = session.key().clone();
        drop(session);
        self.0.remove(&id);
    }

    pub async fn cleanup(&self) {
        let now = UtcDateTime::now();
        self.0.retain(|_, entry| entry.expires >= now);

        tracing::info!("Cleaned up board data")
    }
}

pub type StoreAccessor = Arc<Store>;
