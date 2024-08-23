use std::path::PathBuf;
use axum::{
	{Router, serve},
	body::Body,
	extract::Path,
	http::{header, StatusCode},
	response::{Html, IntoResponse},
	routing::get,
};
use tokio::{
	fs,
	fs::File,
	io::AsyncReadExt,
	net::TcpListener,
};
use tokio_util::io::ReaderStream;

#[tokio::main]
async fn main() {
	let app = Router::new()
		.route("/", get(home))
		.route("/icons/:name", get(get_image))
		.route("/styles/:name", get(get_style))
		.route("/login", get(login))
		.route("/registration", get(registration))
		.fallback(fallback);

	let listener = TcpListener::bind("127.0.0.1:2000").await.unwrap();
	serve(listener, app).await.unwrap();
}

async fn get_final_html(file_name: &str) -> String {
	let contents = fs::read_to_string(file_name).await.unwrap();
	let header = fs::read_to_string("pages/header.html").await.unwrap();
	header.replace("{}", contents.as_str())
}

async fn home() -> impl IntoResponse {
	Html(get_final_html("pages/index.html").await)
}

async fn login() -> impl IntoResponse {
	Html(get_final_html("pages/login.html").await)
}

async fn registration() -> impl IntoResponse {
	Html(get_final_html("pages/registration.html").await)
}

async fn fallback() -> (StatusCode, Html<String>) {
	(StatusCode::NOT_FOUND, Html(get_final_html("pages/not_found.html").await))
}

async fn bad_request() -> (StatusCode, Html<String>) {
	(StatusCode::BAD_REQUEST, Html(get_final_html("pages/bad_request.html").await))
}

async fn get_image(
	Path(name): Path<String>
) -> impl IntoResponse {
	let mut buf = PathBuf::from("icons");
	buf.push(&name);
	let filename = match buf.file_name() {
		Some(name) => name,
		None => return Err(bad_request().await)
	};
	let file = get_file(&buf).await?;
	let content_type = match mime_guess::from_path(&name).first_raw() {
		Some(mime) => mime,
		None => return Err(bad_request().await)
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
		Err(_) => return Err(fallback().await)
	}
}
