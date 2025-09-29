use axum::{
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};

pub mod errors {
    use axum::{BoxError, http::StatusCode, response::IntoResponse};

    #[derive(Debug)]
    enum WebErrorKind {
        Client,
        Internal,
    }

    #[derive(Debug)]
    pub struct WebError {
        kind: WebErrorKind,
        inner: BoxError,
        code: Option<StatusCode>,
    }

    impl WebError {
        pub fn code(mut self, value: StatusCode) -> Self {
            self.code.replace(value);
            self
        }

        pub fn internal(error: BoxError) -> Self {
            WebError {
                kind: WebErrorKind::Internal,
                inner: error,
                code: None,
            }
        }

        pub fn client(error: BoxError) -> Self {
            WebError {
                kind: WebErrorKind::Client,
                inner: error,
                code: None,
            }
        }
    }

    // TODO: better integrate with tower tracing

    impl IntoResponse for WebError {
        fn into_response(self) -> axum::response::Response {
            match self.kind {
                WebErrorKind::Client => {
                    tracing::warn!("Client error: {}", self.inner);
                    (
                        self.code.unwrap_or(StatusCode::BAD_REQUEST),
                        self.inner.to_string(),
                    )
                }
                WebErrorKind::Internal => {
                    tracing::error!("Internal server error: {}", self.inner);
                    (
                        self.code.unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        "Something went wrong".to_string(),
                    )
                }
            }
            .into_response()
        }
    }

    pub type WebResult<T> = Result<T, WebError>;

    impl From<(StatusCode, &'static str)> for WebError {
        fn from(value: (StatusCode, &'static str)) -> Self {
            let (code, string) = value;
            Self::internal(string.into()).code(code)
        }
    }

    // Anyhow integration

    impl From<anyhow::Error> for WebError {
        fn from(value: anyhow::Error) -> Self {
            Self::internal(value.into())
        }
    }

    pub trait AnyhowWebExt {
        fn client_error(self) -> WebError;
    }

    impl AnyhowWebExt for anyhow::Error {
        fn client_error(self) -> WebError {
            WebError::client(self.into())
        }
    }
}

pub mod assets {
    use super::*;

    use rust_embed::Embed;

    #[derive(Embed)]
    #[folder = "src/assets/"]
    struct Assets;

    pub struct StaticFile<T>(pub T);

    impl<T> IntoResponse for StaticFile<T>
    where
        T: Into<String>,
    {
        fn into_response(self) -> Response {
            let path = self.0.into();

            match Assets::get(path.as_str()) {
                Some(content) => (
                    [(header::CONTENT_TYPE, content.metadata.mimetype())],
                    content.data,
                )
                    .into_response(),
                None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
            }
        }
    }

    pub async fn asset_handler(uri: Uri) -> impl IntoResponse {
        StaticFile(uri.path().trim_start_matches('/').to_string())
    }
}

pub mod scheduler {
    pub use time::Duration as Interval;

    pub fn schedule_task<F, Fut>(name: &str, interval: Interval, task_fn: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        tracing::info!("Scheduled {name} to run every {interval}");
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            interval.whole_seconds() as u64,
        ));

        tokio::spawn(async move {
            loop {
                interval.tick().await;
                task_fn().await;
            }
        });
    }
}

pub mod shutdown {
    use tokio::signal;

    pub async fn signal() {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }

        tracing::info!("Shutting down");
    }
}
