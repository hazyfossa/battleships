use rust_embed::Embed;
use axum::{http::{header, StatusCode, Uri}, response::{IntoResponse, Response}};

#[derive(Embed)]
#[folder = "src/assets/"]
#[prefix = "assets"]
struct Assets;

pub struct StaticFile<T>(pub T);

impl<T> IntoResponse for StaticFile<T>
where
  T: Into<String>,
{
  fn into_response(self) -> Response {
    let path = self.0.into();

    match Assets::get(path.as_str()) {
      Some(content) => {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
      }
      None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
  }
}

pub async fn asset_handler(uri: Uri) -> impl IntoResponse {
    StaticFile(uri.path().trim_start_matches('/').to_string())
}