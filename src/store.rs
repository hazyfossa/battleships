use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use dashmap::{
    DashMap, Entry,
    mapref::one::{Ref, RefMut},
};
use rand::Rng;
use time::{Duration, OffsetDateTime, UtcDateTime};
use tower_cookies::{Cookie, Cookies};

use crate::{game::Board, utils::scheduler};

type SessionID = u16;
// TODO: typed cookies
static SESSION_COOKIE_REF: &str = "board";

pub struct Session {
    expires: OffsetDateTime,
    pub board: Board,
}

type SessionRef<'a> = Ref<'a, SessionID, Session>;
type SessionRefMut<'a> = RefMut<'a, SessionID, Session>;

pub struct Store {
    data: DashMap<u16, Session>,
    session_lifetime: Duration,
}

impl<'a> Store {
    pub fn new(session_lifetime: Duration) -> Self {
        Self {
            data: DashMap::new(),
            session_lifetime,
        }
    }

    fn get_vacant_id(&self) -> Option<SessionID> {
        // TODO: optimize
        let mut rng = rand::rng();
        let mut id = rng.random::<SessionID>();

        for _ in 0..SessionID::MAX {
            if self.data.contains_key(&id) {
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

        let session_ref = match self.data.entry(id) {
            Entry::Occupied(_) => bail!("get_vacant_id returned occupied id"),
            Entry::Vacant(entry) => entry.insert(session),
        };

        Ok(session_ref)
    }

    pub fn new_session(&'a self, cookies: &Cookies, board: Board) -> Result<SessionRefMut<'a>> {
        let now = OffsetDateTime::now_utc();
        let expires = now + self.session_lifetime;

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
        Some(self.data.get(&id)?)
    }

    pub async fn remove_session(&self, session: SessionRef<'a>, cookies: &Cookies) {
        cookies.remove(SESSION_COOKIE_REF.into());

        let id = session.key().clone();
        drop(session);
        self.data.remove(&id);
    }

    async fn cleanup(&self) {
        let now = UtcDateTime::now();
        self.data.retain(|_, entry| entry.expires >= now);

        tracing::info!("Cleaned up board data")
    }

    pub fn with_cleanup(self: StoreAccessor) -> StoreAccessor {
        // TODO: it might be useful to cleanup more often under high memory pressure
        // or even schedule individual cleanup tasks per session
        let accessor = self.clone();

        scheduler::schedule_task("Board data cleanup", self.session_lifetime, move || {
            let store = accessor.clone();
            async move {
                store.cleanup().await;
            }
        });
        self
    }
}

pub type StoreAccessor = Arc<Store>;
