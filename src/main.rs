use std::{
	path::PathBuf,
	sync::Arc,
	time::Duration,
};
use axum::{
	{Router, serve}, 
	body::Body, 
	extract::{Path, State},
	Form, 
	http::{header, StatusCode}, 
	response::{Html, IntoResponse, Redirect}, 
	routing::get,
};
use base16ct::lower;
use rand::prelude::StdRng;
use sha2::{
	Digest,
	Sha512
};
use sqlx::{
	PgPool,
	postgres::PgPoolOptions,
	Row
};
use tokio::{
	fs,
	fs::File,
	io::AsyncReadExt,
	net::TcpListener,
};
use tokio_util::io::ReaderStream;
use serde::Deserialize;
use rand::{
	RngCore,
	SeedableRng
};

struct SharedStateStruct {
	pool: PgPool
}

#[tokio::main]
async fn main() {
	let pool = PgPoolOptions::new()
		.max_connections(20)
		.min_connections(2)
		.idle_timeout(Duration::new(60, 0))
		.connect("postgres://postgres:293658@localhost/web_page_db").await.unwrap();
	let shared_state = Arc::new(SharedStateStruct{pool});
	
	let app = Router::new()
		.route("/", get(home))
		.route("/icons/:name", get(get_image))
		.route("/styles/:name", get(get_style))
		.route("/scripts/:name", get(get_script))
		.route("/login", get(login).post(post_login))
		.route("/registration", get(registration).post(post_registration))
		.with_state(shared_state)
		.fallback(fallback);

	let listener = TcpListener::bind("127.0.0.1:2000").await.unwrap();
	serve(listener, app).await.unwrap();
}

async fn get_final_html(file_name: &str) -> String {
	let contents = fs::read_to_string(file_name).await.unwrap();
	let header = fs::read_to_string("pages/header.html").await.unwrap();
	header.replace("{}", contents.as_str())
}

async fn home() -> Html<String> {
	Html(get_final_html("pages/index.html").await)
}

async fn login() -> Html<String> {
	Html(get_final_html("pages/login.html").await)
}

async fn registration() -> Html<String> {
	Html(get_final_html("pages/registration.html").await)
}

#[derive(Deserialize)]
struct UserInfo {
	email: String,
	password: String,
}

async fn post_login(State(state): State<Arc<SharedStateStruct>>, Form(payload): Form<UserInfo>) -> impl IntoResponse {
	let mut hasher = Sha512::new();
	hasher.update(&payload.password);
	let hash = hasher.finalize();
	let hex_hash = lower::encode_string(&hash);
	
	let account = sqlx::query("SELECT user_id FROM accounts WHERE email = $1 AND password_hash = $2")
		.bind(&payload.email)
		.bind(hex_hash)
		.fetch_optional(&state.pool)
		.await.unwrap();
	
	match account {
		Some(account) => {
			let mut buf = [0; 64];
			let cookie = loop {
				let mut rng = StdRng::from_entropy();
				rng.fill_bytes(&mut buf);
				let cookie = lower::encode_string(&buf);
				
				let result = sqlx::query("SELECT user_id FROM accounts WHERE cookie = $1")
					.bind(&cookie)
					.fetch_optional(&state.pool)
					.await.unwrap();
				
				if let None = result {
					break cookie
				}
			};
			
			let id: i32 = account.get("user_id");
			let result = sqlx::query("UPDATE accounts SET cookie = $1 WHERE user_id = $2")
				.bind(&cookie)
				.bind(id)
				.execute(&state.pool)
				.await;
			
			match result { 
				Ok(_) => Ok(([(header::SET_COOKIE, format!("SECURITY-COOKIE={cookie}"))], Redirect::to("/"))),
				Err(_) => Err(login().await)
			}
		}
		None => {
			Err(login().await)
		}
	}
}

async fn post_registration(State(_state): State<Arc<SharedStateStruct>>, Form(_payload): Form<UserInfo>) -> impl IntoResponse {
	registration().await
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

	let headers = [(header::CONTENT_TYPE, "text/css".to_string())];
	let body = read_file_to_string(&buf).await?;

	Ok((headers, body))
}

async fn get_script(
	Path(name): Path<String>
) -> Result<impl IntoResponse, (StatusCode, Html<String>)> {
	let mut buf = PathBuf::from("scripts");
	buf.push(&name);

	let headers = [(header::CONTENT_TYPE, "text/javascript".to_string())];
	let body = read_file_to_string(&buf).await?;

	Ok((headers, body))
}

async fn read_file_to_string(buf: &PathBuf) -> Result<String, (StatusCode, Html<String>)> {
	let mut file = get_file(&buf).await?;

	let mut body = String::new();
	file.read_to_string(&mut body).await.unwrap();
	
	Ok(body)
}

async fn get_file(path: &PathBuf) -> Result<File, (StatusCode, Html<String>)> {
	match File::open(&path).await {
		Ok(file) => Ok(file),
		Err(_) => Err(fallback().await)
	}
}
