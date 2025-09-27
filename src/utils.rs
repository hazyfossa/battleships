use axum::{
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};

pub mod errors {
    use super::*;
    pub struct InternalError(anyhow::Error);
    pub type Fallible<T> = Result<T, InternalError>;

    // Tell axum how to convert `AppError` into a response.
    impl IntoResponse for InternalError {
        fn into_response(self) -> Response {
            // TODO: do not expose full error to client
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Something went wrong: {}", self.0),
            )
                .into_response()
        }
    }

    impl<E> From<E> for InternalError
    where
        E: Into<anyhow::Error>,
    {
        fn from(err: E) -> Self {
            Self(err.into())
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
    use tracing::{Level, event};

    pub fn schedule_task<F, Fut>(name: &str, interval: Interval, task_fn: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        event!(Level::INFO, "Scheduled {name} to run every {interval}");
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
