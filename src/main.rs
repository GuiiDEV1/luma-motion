#![cfg_attr(windows, windows_subsystem = "windows")]
mod config;
#[cfg(windows)]
mod desktop;
mod engine;
mod gpu;
mod motion;

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use engine::{Meta, Settings};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

type ApiError = (StatusCode, Json<Value>);
fn error(status: StatusCode, message: impl ToString) -> ApiError {
    (status, Json(json!({"detail":message.to_string()})))
}
#[derive(Clone)]
struct Media {
    path: PathBuf,
    name: String,
    meta: Meta,
}
#[derive(Clone, Serialize)]
struct Job {
    id: String,
    status: String,
    progress: f64,
    preview: bool,
    name: String,
    meta: Option<Meta>,
    error: Option<String>,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    cancel: Arc<AtomicBool>,
}
#[derive(Clone)]
struct AppState {
    work: PathBuf,
    config_path: PathBuf,
    config: Arc<Mutex<config::Config>>,
    media: Arc<Mutex<HashMap<String, Media>>>,
    jobs: Arc<Mutex<HashMap<String, Job>>>,
}

async fn local_only(request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    if !["127.0.0.1", "localhost"].contains(&host)
        || origin.is_some_and(|v| {
            v != format!(
                "http://{}",
                request
                    .headers()
                    .get(header::HOST)
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("")
            )
        })
    {
        return error(
            StatusCode::FORBIDDEN,
            "Acesso disponível apenas no aplicativo local.",
        )
        .into_response();
    }
    next.run(request).await
}
async fn health() -> Json<Value> {
    let ready = tokio::task::spawn_blocking(|| {
        engine::command("ffmpeg").arg("-version").output().is_ok()
            && engine::command("ffprobe").arg("-version").output().is_ok()
    })
    .await
    .unwrap_or(false);
    let (gpu, nvenc) =
        tokio::task::spawn_blocking(|| (gpu::availability(), engine::nvenc_availability()))
            .await
            .unwrap_or_else(|e| (Err(e.to_string()), Err(e.to_string())));
    Json(
        json!({"ready":ready,"engine":"Rust · CPU / GPU", "gpu":{"ready":gpu.is_ok(),"name":gpu.as_ref().ok(),"error":gpu.as_ref().err()},"nvenc":{"ready":nvenc.is_ok(),"error":nvenc.err()}}),
    )
}
async fn upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let mut field = multipart
        .next_field()
        .await
        .map_err(|e| error(StatusCode::BAD_REQUEST, e))?
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Selecione um vídeo."))?;
    let name = field.file_name().unwrap_or("video.mp4").to_string();
    let ext = std::path::Path::new(&name)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["mp4", "mov", "mkv", "avi", "webm", "m4v"].contains(&ext.as_str()) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Importe MP4, MOV, MKV, AVI ou WebM.",
        ));
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = state.work.join(format!("{id}.{ext}"));
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    loop {
        match field.chunk().await {
            Ok(Some(chunk)) => {
                if let Err(e) = file.write_all(&chunk).await {
                    drop(file);
                    let _ = tokio::fs::remove_file(&path).await;
                    return Err(error(StatusCode::INTERNAL_SERVER_ERROR, e));
                }
            }
            Ok(None) => break,
            Err(e) => {
                drop(file);
                let _ = tokio::fs::remove_file(&path).await;
                return Err(error(StatusCode::BAD_REQUEST, e));
            }
        }
    }
    drop(file);
    let probe_path = path.clone();
    let info = tokio::task::spawn_blocking(move || engine::probe(&probe_path))
        .await
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let meta = match info {
        Ok(meta) => meta,
        Err(e) => {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(error(StatusCode::BAD_REQUEST, e));
        }
    };
    state.media.lock().unwrap().insert(
        id.clone(),
        Media {
            path,
            name: name.clone(),
            meta: meta.clone(),
        },
    );
    let mut value = serde_json::to_value(meta).unwrap();
    value["id"] = json!(id);
    value["name"] = json!(name);
    value["url"] = json!(format!("/api/media/{id}"));
    Ok(Json(value))
}
async fn serve_file(path: PathBuf, request: Request) -> Response {
    match ServeFile::new(path).oneshot(request).await {
        Ok(response) => response.map(Body::new),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}
async fn original(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let path = state
        .media
        .lock()
        .unwrap()
        .get(&id)
        .map(|m| m.path.clone())
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Vídeo não encontrado."))?;
    Ok(serve_file(path, request).await)
}
async fn create_job(
    State(state): State<AppState>,
    Json(settings): Json<Settings>,
) -> Result<Json<Value>, ApiError> {
    let media = state
        .media
        .lock()
        .unwrap()
        .get(&settings.media_id)
        .cloned()
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Importe um vídeo primeiro."))?;
    settings
        .validate(&media.meta)
        .map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let ext = if settings.codec == "prores"
        && !settings.preview
        && !settings.compression_enabled
        && !settings.compress_only
    {
        "mov"
    } else {
        "mp4"
    };
    let output = state.work.join(format!("{id}.{ext}"));
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = state.jobs.lock().unwrap();
        if jobs
            .values()
            .any(|j| ["queued", "running"].contains(&j.status.as_str()))
        {
            return Err(error(
                StatusCode::CONFLICT,
                "Aguarde a renderização ou cancele a atual.",
            ));
        }
        let name = std::path::Path::new(&media.name)
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("video");
        jobs.insert(
            id.clone(),
            Job {
                id: id.clone(),
                status: "queued".into(),
                progress: 0.0,
                preview: settings.preview,
                name: format!("{name}-luma.{ext}"),
                path: output.clone(),
                cancel: cancel.clone(),
                meta: None,
                error: None,
            },
        );
    }
    let job_id = id.clone();
    tokio::task::spawn_blocking(move || {
        state.jobs.lock().unwrap().get_mut(&job_id).unwrap().status = "running".into();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            engine::render(
                &media.path,
                &output,
                &settings,
                cancel.clone(),
                |progress| {
                    state
                        .jobs
                        .lock()
                        .unwrap()
                        .get_mut(&job_id)
                        .unwrap()
                        .progress = (progress * 10.0).round() / 10.0;
                },
            )
        }))
        .unwrap_or_else(|_| Err("O motor encontrou um erro inesperado.".into()));
        let mut jobs = state.jobs.lock().unwrap();
        let job = jobs.get_mut(&job_id).unwrap();
        if cancel.load(Ordering::Relaxed) {
            job.status = "cancelled".into();
            let _ = std::fs::remove_file(&output);
        } else {
            match result {
                Ok(meta) => {
                    job.status = "done".into();
                    job.progress = 100.0;
                    job.meta = Some(meta);
                }
                Err(e) => {
                    job.status = "error".into();
                    job.error = Some(e);
                    let _ = std::fs::remove_file(&output);
                }
            }
        }
    });
    Ok(Json(json!({"id":id})))
}
async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Job>, ApiError> {
    state
        .jobs
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Renderização não encontrada."))
}
async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let jobs = state.jobs.lock().unwrap();
    let job = jobs
        .get(&id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Renderização não encontrada."))?;
    job.cancel.store(true, Ordering::Relaxed);
    Ok(Json(json!({"ok":true})))
}
async fn video(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    request: Request,
) -> Result<Response, ApiError> {
    let job = state
        .jobs
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .filter(|j| j.status == "done")
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Vídeo ainda não está pronto."))?;
    let mut response = serve_file(job.path, request).await;
    if query.get("download").is_some_and(|v| v == "true") {
        let safe: String = job
            .name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "-_. ".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if let Ok(value) = format!("attachment; filename=\"{safe}\"").parse() {
            response
                .headers_mut()
                .insert(header::CONTENT_DISPOSITION, value);
        }
    }
    Ok(response)
}

async fn get_config(State(state): State<AppState>) -> Json<config::Config> {
    Json(state.config.lock().unwrap().clone())
}
async fn export_config(State(state): State<AppState>) -> Response {
    let config = state.config.lock().unwrap().clone();
    let mut value = serde_json::to_value(&config).unwrap();
    if let Some(settings) = value["settings"].as_object_mut() {
        for key in ["media_id", "start", "preview"] {
            settings.remove(key);
        }
    }
    let name: String = config
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut response = (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        serde_json::to_string_pretty(&value).unwrap(),
    )
        .into_response();
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{name}.luma.json\"")
            .parse()
            .unwrap(),
    );
    response
}
async fn save_config(
    State(state): State<AppState>,
    Json(config): Json<config::Config>,
) -> Result<Json<config::Config>, ApiError> {
    let config = config
        .validated()
        .map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
    let mut saved = state.config.lock().unwrap();
    let temp = state.config_path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(&config)
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    std::fs::write(&temp, bytes)
        .and_then(|_| std::fs::rename(&temp, &state.config_path))
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    *saved = config.clone();
    Ok(Json(config))
}
async fn asset(request: Request) -> Response {
    let (mime, content) = match request.uri().path() {
        "/" | "/index.html" => (
            "text/html; charset=utf-8",
            include_str!("../web/index.html"),
        ),
        "/style.css" => ("text/css; charset=utf-8", include_str!("../web/style.css")),
        "/app.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/app.js"),
        ),
        "/icons.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/icons.js"),
        ),
        "/desktop.css" => (
            "text/css; charset=utf-8",
            include_str!("../web/desktop.css"),
        ),
        "/shell.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/shell.js"),
        ),
        "/configs.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/configs.js"),
        ),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, mime)], content).into_response()
}
fn router(data: &std::path::Path) -> (Router, Arc<Mutex<HashMap<String, Job>>>) {
    let work = data.join("workspace");
    std::fs::create_dir_all(&work).expect("Não foi possível criar a pasta de vídeos.");
    let config_path = data.join("config.json");
    let config = std::fs::read(&config_path)
        .ok()
        .and_then(|b| serde_json::from_slice::<config::Config>(&b).ok())
        .and_then(|c| c.validated().ok())
        .unwrap_or_default();
    let state = AppState {
        work,
        config_path,
        config: Arc::new(Mutex::new(config)),
        media: Default::default(),
        jobs: Default::default(),
    };
    let jobs = state.jobs.clone();
    let router = Router::new()
        .route("/api/health", get(health))
        .route("/api/config", get(get_config).put(save_config))
        .route("/api/config/export", get(export_config))
        .route("/api/media", post(upload))
        .route("/api/media/{id}", get(original))
        .route("/api/jobs", post(create_job))
        .route("/api/jobs/{id}", get(status))
        .route("/api/jobs/{id}/cancel", post(cancel))
        .route("/api/jobs/{id}/video", get(video))
        .fallback(asset)
        .layer(DefaultBodyLimit::disable())
        .layer(middleware::from_fn(local_only))
        .with_state(state);
    (router, jobs)
}
fn main() {
    let web = std::env::args().any(|a| a == "--web");
    let data = if web {
        std::env::current_dir().unwrap()
    } else {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| ".".into()))
            .join("LumaMotion")
    };
    std::fs::create_dir_all(&data).expect("Não foi possível criar a pasta de dados.");
    let (app, jobs) = router(&data);
    let listener = std::net::TcpListener::bind("127.0.0.1:8765")
        .or_else(|e| {
            if web {
                Err(e)
            } else {
                std::net::TcpListener::bind("127.0.0.1:0")
            }
        })
        .expect("Porta ocupada. Feche a versão web anterior.");
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let run = move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), app)
                .await
                .unwrap();
        });
    };
    if web {
        println!("Luma Motion · {}", url);
        run();
        return;
    }
    std::thread::spawn(run);
    #[cfg(windows)]
    if let Err(message) = desktop::run(&url, &data, move || {
        !jobs
            .lock()
            .unwrap()
            .values()
            .any(|j| ["queued", "running"].contains(&j.status.as_str()))
    }) {
        rfd::MessageDialog::new()
            .set_title("Luma Motion")
            .set_description(message)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
    #[cfg(not(windows))]
    {
        println!("Abra {url}");
        std::thread::park();
    }
}
