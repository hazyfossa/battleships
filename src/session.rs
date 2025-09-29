use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use axum::{
    extract::{FromRef, FromRequestParts},
    http::StatusCode,
};
use dashmap::{
    DashMap, Entry,
    mapref::one::{Ref, RefMut},
};
use time::{Duration, OffsetDateTime, UtcDateTime};
use tower_cookies::{Cookie, Cookies};
use uuid::Uuid;

use crate::{
    game::Board,
    utils::{
        errors::{AnyhowWebExt, WebError, WebResult},
        scheduler,
    },
};

type SessionID = Uuid;
// TODO: typed cookies
static SESSION_COOKIE_REF: &str = "board";

pub struct Session {
    expires: OffsetDateTime,
    pub board: Board,
}

type SessionRef<'a> = Ref<'a, SessionID, Session>;
type SessionRefMut<'a> = RefMut<'a, SessionID, Session>;

pub struct Store {
    data: DashMap<SessionID, Session>,
    session_lifetime: Duration,
}

impl<'a> Store {
    pub fn new(session_lifetime: Duration) -> Self {
        Self {
            data: DashMap::new(),
            session_lifetime,
        }
    }

    fn insert(&'a self, session: Session) -> Result<SessionRefMut<'a>> {
        let id = SessionID::now_v7();

        let session_ref = match self.data.entry(id) {
            Entry::Occupied(_) => bail!("UUID collision?!"),
            Entry::Vacant(entry) => entry.insert(session),
        };

        Ok(session_ref)
    }

    fn get(&'a self, id: &SessionID) -> Option<SessionRef<'a>> {
        self.data.get(id)
    }

    async fn delete(&self, session: SessionRef<'a>) {
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

type StoreAccessor = Arc<Store>;

pub struct SessionManager {
    store: StoreAccessor,
    cookies: Cookies,
}

impl<'a> SessionManager {
    pub fn create(&'a self, board: Board) -> Result<SessionRefMut<'a>> {
        let now = OffsetDateTime::now_utc();
        let expires = now + self.store.session_lifetime;

        let session = self.store.insert(Session { expires, board })?;
        let id = session.key();

        self.cookies.add(
            Cookie::build((SESSION_COOKIE_REF, id.to_string()))
                .expires(expires)
                .build(),
        );

        tracing::info!("New session created: {}", id);
        Ok(session)
    }

    pub fn current(&'a self) -> Option<SessionRef<'a>> {
        // TODO: maybe propagate parse error
        let id = &self.cookies.get(SESSION_COOKIE_REF)?.value().parse().ok()?;
        self.store.get(id)
    }

    pub async fn delete(&'a self, handle: SessionRef<'a>) {
        self.cookies.remove(SESSION_COOKIE_REF.into());
        self.store.delete(handle).await;
    }

    pub fn current_exists(&self) -> bool {
        // TODO: there is a flaw with this approach:
        // if the cookie is invalid or we dropped store between client requests
        // the client will see a non-functional continue game button
        self.cookies.get(SESSION_COOKIE_REF).is_some()
    }
}

pub trait SessionOptionExt<'a> {
    fn require(self) -> WebResult<SessionRef<'a>>;
}

impl<'a> SessionOptionExt<'a> for Option<SessionRef<'a>> {
    fn require(self) -> WebResult<SessionRef<'a>> {
        self.ok_or(
            anyhow!("Session not found")
                .client_error()
                .code(StatusCode::UNAUTHORIZED),
        )
    }
}

impl<S> FromRequestParts<S> for SessionManager
where
    S: Send + Sync,
    StoreAccessor: FromRef<S>,
{
    type Rejection = WebError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let store = StoreAccessor::from_ref(state);
        let cookies = Cookies::from_request_parts(parts, state).await?;

        Ok(Self { store, cookies })
    }
}
