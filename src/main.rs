use std::{
	path::PathBuf,
	sync::Arc,
	time::Duration,
	io::Error,
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
use axum_extra::extract::cookie::{
	Cookie,
	CookieJar,
};
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
use rand::{
	RngCore,
	SeedableRng,
	prelude::StdRng
};
use tokio_util::io::ReaderStream;
use time::OffsetDateTime;
use serde::Deserialize;
use base16ct::lower;
use email_address::EmailAddress;

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
		.route("/logout", get(logout))
		.fallback(fallback)
		.with_state(shared_state);

	let listener = TcpListener::bind("127.0.0.1:2000").await.unwrap();
	serve(listener, app).await.unwrap();
}

async fn get_final_html(
	file_name: &str, 
	jar: CookieJar, 
	state: Arc<SharedStateStruct>
) -> String {
	let main_body = fs::read_to_string(file_name).await.unwrap();
	let header = fs::read_to_string("pages/header.html").await.unwrap();
	let auth = match jar.get("SECURITY-COOKIE") {
		Some(val) => {
			let val = val.value();
			let result = sqlx::query("SELECT user_id FROM accounts WHERE cookie = $1")
				.bind(val)
				.fetch_optional(&state.pool)
				.await.unwrap();
			
			match result {
				Some(_) => fs::read_to_string("pages/auth.html").await.unwrap(),
				None => fs::read_to_string("pages/not_auth.html").await.unwrap()
			}
		}
		None => fs::read_to_string("pages/not_auth.html").await.unwrap()
	};
	header.replace("{main_body}", main_body.as_str()).replace("{auth}", auth.as_str())
}

async fn home(
	jar: CookieJar, 
	State(state): State<Arc<SharedStateStruct>>
) -> Html<String> {
	Html(get_final_html("pages/index.html", jar, state).await)
}

async fn login(
	jar: CookieJar, 
	State(state): State<Arc<SharedStateStruct>>
) -> Html<String> {
	Html(get_final_html("pages/login.html", jar, state).await)
}

async fn registration(
	jar: CookieJar, 
	State(state): State<Arc<SharedStateStruct>>
) -> Html<String> {
	Html(get_final_html("pages/registration.html", jar, state).await)
}

async fn logout(jar: CookieJar) -> impl IntoResponse {
	let mut cookie = Cookie::new("SECURITY-COOKIE", "");
	cookie.set_secure(true);
	cookie.set_expires(OffsetDateTime::UNIX_EPOCH);
	(jar.add(cookie), Redirect::to("/"))
}

#[derive(Deserialize)]
struct UserInfo {
	email: String,
	password: String,
}

async fn post_login(
	jar: CookieJar, 
	State(state): State<Arc<SharedStateStruct>>, 
	Form(payload): Form<UserInfo>
) -> impl IntoResponse {
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
				
				if result.is_none() {
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
				Ok(_) => {
					let mut cookie = Cookie::new("SECURITY-COOKIE", cookie);
					cookie.set_secure(true);
					let mut now = OffsetDateTime::now_utc();
					now += Duration::new(31104000, 0);
					cookie.set_expires(now);
					
					Ok((jar.add(cookie), Redirect::to("/")))
				}
				Err(_) => Err(login(jar, State(state)).await)
			}
		}
		None => Err(login(jar, State(state)).await)
	}
}

async fn post_registration(
	jar: CookieJar, 
	State(state): State<Arc<SharedStateStruct>>, 
	Form(payload): Form<UserInfo>
) -> impl IntoResponse {
	if !EmailAddress::is_valid(&payload.email) {
		return Err(registration(jar, State(state)).await)
	}
	
	let account = sqlx::query("SELECT user_id FROM accounts WHERE email = $1")
		.bind(&payload.email)
		.fetch_optional(&state.pool)
		.await.unwrap();
	
	match account {
		Some(_) => Err(registration(jar, State(state)).await),
		None => {
			let mut hasher = Sha512::new();
			hasher.update(&payload.password);
			let hash = hasher.finalize();
			let hex_hash = lower::encode_string(&hash);
			
			let result = sqlx::query("INSERT INTO accounts(email, password_hash) VALUES ($1, $2)")
				.bind(&payload.email)
				.bind(hex_hash)
				.execute(&state.pool)
				.await;
			
			match result {
				Ok(_) => Ok(Redirect::to("/login")),
				Err(_) => Err(registration(jar, State(state)).await)
			}
		}
	}
}

async fn fallback(
	jar: CookieJar, 
	State(state): State<Arc<SharedStateStruct>>
) -> (StatusCode, Html<String>) {
	(StatusCode::NOT_FOUND, Html(get_final_html("pages/not_found.html", jar, state).await))
}

async fn bad_request(
	jar: CookieJar, 
	State(state): State<Arc<SharedStateStruct>>
) -> (StatusCode, Html<String>) {
	(StatusCode::BAD_REQUEST, Html(get_final_html("pages/bad_request.html", jar, state).await))
}

async fn get_image(
	jar: CookieJar,
	State(state): State<Arc<SharedStateStruct>>,
	Path(name): Path<String>
) -> impl IntoResponse {
	let mut buf = PathBuf::from("icons");
	buf.push(&name);
	let filename = match buf.file_name() {
		Some(name) => name,
		None => return Err(bad_request(jar, State(state)).await)
	};
	let file = match get_file(&buf).await {
		Ok(file) => file,
		Err(_) => return Err(fallback(jar, State(state)).await)
	};
	let content_type = match mime_guess::from_path(&name).first_raw() {
		Some(mime) => mime,
		None => return Err(bad_request(jar, State(state)).await)
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
	jar: CookieJar,
	State(state): State<Arc<SharedStateStruct>>,
	Path(name): Path<String>
) -> Result<impl IntoResponse, (StatusCode, Html<String>)> {
	let mut buf = PathBuf::from("styles");
	buf.push(&name);

	let headers = [(header::CONTENT_TYPE, "text/css".to_string())];
	let body = match read_file_to_string(&buf).await { 
		Ok(body) => body,
		Err(_) => {
			return Err(fallback(jar, State(state)).await)
		}
	};

	Ok((headers, body))
}

async fn get_script(
	jar: CookieJar,
	State(state): State<Arc<SharedStateStruct>>,
	Path(name): Path<String>
) -> Result<impl IntoResponse, (StatusCode, Html<String>)> {
	let mut buf = PathBuf::from("scripts");
	buf.push(&name);

	let headers = [(header::CONTENT_TYPE, "text/javascript".to_string())];
	let body = match read_file_to_string(&buf).await {
		Ok(body) => body,
		Err(_) => {
			return Err(fallback(jar, State(state)).await)
		}
	};

	Ok((headers, body))
}

async fn read_file_to_string(buf: &PathBuf) -> Result<String, Error> {
	let mut file = get_file(buf).await?;

	let mut body = String::new();
	file.read_to_string(&mut body).await.unwrap();
	
	Ok(body)
}

async fn get_file(path: &PathBuf) -> Result<File, Error> {
	File::open(&path).await
}
