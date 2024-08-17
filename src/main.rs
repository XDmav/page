use std::path::PathBuf;
use axum::{Router, serve};
use axum::body::Body;
use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio_util::io::ReaderStream;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(home))
        .route("/icons/:name", get(get_image))
        .route("/styles/:name", get(get_style));
    
    let listener = TcpListener::bind("127.0.0.1:2000").await.unwrap();
    serve(listener, app).await.unwrap();
}

async fn home() -> impl IntoResponse {
    let contents = fs::read_to_string("pages/index.html").await.unwrap();
    Html(contents)
}
async fn get_image(
    Path(name): Path<String>
) -> impl IntoResponse {
    let mut buf = PathBuf::from("icons");
    buf.push(&name);
    let filename = match buf.file_name() {
        Some(name) => name,
        None => return Err((StatusCode::BAD_REQUEST, Html("File name couldn't be determined".to_string())))
    };
    let file = get_file(&buf).await?;
    let content_type = match mime_guess::from_path(&name).first_raw() {
        Some(mime) => mime,
        None => return Err((StatusCode::BAD_REQUEST, Html("MIME Type couldn't be determined".to_string())))
    };

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let headers = [
        (header::CONTENT_TYPE, content_type.to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{:?}\"", filename),
        ),
    ];
    Ok((headers, body))
}

async fn get_style(
    Path(name): Path<String>
) -> Result<impl IntoResponse, (StatusCode, Html<String>)> {
    let mut buf = PathBuf::from("styles");
    buf.push(&name);
    let mut file = get_file(&buf).await?;

    let mut body = String::new();
    file.read_to_string(&mut body).await.unwrap();

    let headers = [(header::CONTENT_TYPE, "text/css".to_string())];
    Ok((headers, body))
}

async fn get_file(path: &PathBuf) -> Result<File, (StatusCode, Html<String>)> {
    match File::open(&path).await {
        Ok(file) => Ok(file),
        Err(_) => {
            let contents = fs::read_to_string("pages/not_found.html").await.unwrap();
            return Err((StatusCode::NOT_FOUND, Html(contents)))
        }
    }
}
