//! The Manager's commands, answered over HTTP. Names and arguments are those of the desktop app's Tauri commands
//! (`src-tauri/src/lib.rs`), so `src/lib/api.ts` calls them unchanged. Tauri hands the interface's camelCase argument
//! names to snake_case parameters; the argument structs below do the same with `rename_all = "camelCase"`.
//!
//! This prototype covers servers, start/stop and status, logs, accounts, population and the Docker install. Every
//! other command answers "not available in the web version yet".

use std::path::{Path, PathBuf};
use std::sync::Arc;

use coa_core::docker::{self, install as docker_install};
use coa_core::download::Cancel;
use coa_core::driver::{self, Verb};
use coa_core::error::UiError;
use coa_core::layout::{self, Classification};
use coa_core::pkgsource::Source;
use coa_core::process::{self, Observed, ServiceState};
use coa_core::ra::Ra;
use coa_core::registry::{metadata_dir_for, InstallKind, InstallMeta, MetaDir};
use coa_core::{Error, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::{AppState, BusyGuard};

/// The commands this service answers (also listed by `GET /api/info`).
pub const SUPPORTED: &[&str] = &[
    "list_servers",
    "scan_server",
    "add_server",
    "forget_server",
    "server_status",
    "start_server",
    "stop_server",
    "force_stop_server",
    "console_tail",
    "list_accounts",
    "create_account",
    "account_set_password",
    "account_set_access",
    "get_population",
    "install_environment",
    "default_install_dir",
    "install_preflight",
    "install_requirements",
    "install_new",
    "cancel_install",
    "game_data_folder",
    // Desktop-only preferences the interface reads on start: fixed answers in a browser.
    "close_to_tray",
    "clear_game_cache_on_play",
    "browse_enabled",
    "manager_installing",
];

pub enum CallError {
    Unknown,
    Ui(UiError),
}

impl From<Error> for CallError {
    fn from(e: Error) -> Self {
        CallError::Ui(e.into())
    }
}

type Answer = std::result::Result<Value, CallError>;

fn args<T: DeserializeOwned>(v: Value) -> std::result::Result<T, CallError> {
    serde_json::from_value(v).map_err(|e| Error::Invalid(format!("Invalid arguments: {e}")).into())
}

fn answer<T: Serialize>(v: T) -> Answer {
    serde_json::to_value(v).map_err(|e| Error::Invalid(e.to_string()).into())
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> std::result::Result<T, CallError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| Error::Invalid(e.to_string()))?
        .map_err(Into::into)
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
struct PathArg {
    path: String,
}

#[derive(Deserialize)]
struct ConsoleArgs {
    id: String,
    source: coa_core::console::Source,
    filter: Option<String>,
    lines: Option<usize>,
}

#[derive(Deserialize)]
struct PopulationArgs {
    id: String,
    realm: Option<coa_core::realms::Mode>,
}

#[derive(Deserialize)]
struct CreateAccount {
    id: String,
    username: String,
    password: String,
    administrator: bool,
}

#[derive(Deserialize)]
struct SetPassword {
    id: String,
    name: String,
    password: String,
}

#[derive(Deserialize)]
struct SetAccess {
    id: String,
    name: String,
    level: u8,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreflightArgs {
    dest: String,
    needed: Option<u64>,
    game_data: Option<String>,
}

#[derive(Deserialize)]
struct RequirementsArgs {
    package: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallArgs {
    dest: String,
    package: Option<String>,
    game_data: Option<String>,
}

#[derive(Serialize)]
pub struct ServerSummary {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Serialize)]
struct StatusView {
    observed: Observed,
    busy: bool,
    path_exists: bool,
    scheduled_restart: bool,
}

#[derive(Serialize)]
struct Environment {
    flavor: coa_core::platform::Flavor,
    default_dir: String,
    docker_problem: Option<String>,
}

#[derive(Serialize)]
struct InstallRequirements {
    download_bytes: u64,
    unpacked_bytes: u64,
    version: String,
}

fn summary(id: String, path: PathBuf) -> ServerSummary {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Server".into());
    ServerSummary {
        id,
        name,
        path: path.to_string_lossy().into_owned(),
    }
}

fn meta_dir(root: &Path) -> Result<PathBuf> {
    let dir = metadata_dir_for(root)?;
    if dir.join("install.json").is_file() {
        Ok(dir)
    } else {
        Err(Error::Invalid("This server has not been added to the Manager yet.".into()))
    }
}

fn default_install_dir() -> String {
    match std::env::var("HOME").ok().as_deref().map(|h| h.trim_end_matches('/')) {
        Some(h) if !h.is_empty() => format!("{h}/CoaServer"),
        _ => "/srv/coa".into(),
    }
}

/// Where the Linux package comes from: the interface's choice, else `COA_PACKAGE_SOURCE` (a folder or an http(s)
/// address, for example the `linux-unsigned` release of a build fork).
fn package_source(custom: Option<String>) -> Result<Source> {
    let pick = custom
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("COA_PACKAGE_SOURCE").ok().filter(|s| !s.trim().is_empty()));
    match pick {
        Some(p) if p.to_ascii_lowercase().starts_with("http") => Ok(Source::Url(p)),
        Some(p) => Ok(Source::Dir(PathBuf::from(p))),
        None => Err(Error::Invalid(
            "Choose the Linux server package (a folder or an address): none is published yet.".into(),
        )),
    }
}

/// The key a package must be signed with. Unlike the desktop app, the web service may be told to trust another key
/// (`COA_TRUSTED_KEY`), because whoever runs it may build and sign the Linux package themselves.
fn trusted_key() -> String {
    std::env::var("COA_TRUSTED_KEY")
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| coa_core::signing::EMBEDDED_PUBLIC_KEY.to_string())
}

pub async fn dispatch(state: &Arc<AppState>, command: &str, a: Value) -> Answer {
    match command {
        "list_servers" => answer(
            state
                .registry()
                .list()?
                .into_iter()
                .map(|(id, p)| summary(id, p))
                .collect::<Vec<_>>(),
        ),
        "scan_server" => {
            let PathArg { path } = args(a)?;
            answer(blocking(move || layout::scan(Path::new(&path))).await?)
        }
        "add_server" => {
            let PathArg { path } = args(a)?;
            add_server(state.clone(), path).await
        }
        "forget_server" => {
            // Removes only the registry entry; no files are deleted.
            let Id { id } = args(a)?;
            answer(state.registry().unregister(&id)?)
        }
        "server_status" => {
            let Id { id } = args(a)?;
            let root = state.path_of(&id)?;
            let busy = state.busy.lock().map(|b| b.contains(&id)).unwrap_or(false);
            let path_exists = root.is_dir();
            let (observed, scheduled_restart) = blocking(move || {
                let ports = layout::read_ports(&root);
                Ok((
                    process::observe(&root, &ports),
                    coa_core::launcher::schedule::under_way(&root),
                ))
            })
            .await?;
            answer(StatusView {
                observed,
                busy,
                path_exists,
                scheduled_restart,
            })
        }
        "start_server" => {
            let Id { id } = args(a)?;
            start_server(state, id).await
        }
        "stop_server" => {
            let Id { id } = args(a)?;
            let root = state.path_of(&id)?;
            let _guard = BusyGuard::acquire(state, &id)?;
            answer(blocking(move || driver::run(&root, Verb::StopAll)).await?)
        }
        "force_stop_server" => {
            let Id { id } = args(a)?;
            let root = state.path_of(&id)?;
            let _guard = BusyGuard::acquire(state, &id)?;
            answer(blocking(move || driver::force_stop(&root)).await?)
        }
        "console_tail" => {
            let ConsoleArgs { id, source, filter, lines } = args(a)?;
            let root = state.path_of(&id)?;
            let manager_log = state.data_dir.join("logs").join("manager.log");
            let lines = lines.unwrap_or(300).min(2000);
            answer(
                blocking(move || {
                    // A Docker server's database writes to its container's output, not to a file.
                    if docker::is_docker(&root) && matches!(source, coa_core::console::Source::Database) {
                        return docker::logs::database_log(&root, filter.as_deref(), lines);
                    }
                    let path = coa_core::console::log_path(&root, &manager_log, source);
                    coa_core::console::tail(&path, filter.as_deref(), lines)
                })
                .await?,
            )
        }
        "list_accounts" => {
            let Id { id } = args(a)?;
            let root = state.path_of(&id)?;
            answer(blocking(move || coa_core::accounts::list(&root)).await?)
        }
        "create_account" => {
            let CreateAccount { id, username, password, administrator } = args(a)?;
            let root = state.path_of(&id)?;
            let _guard = BusyGuard::acquire(state, &id)?;
            answer(
                blocking(move || {
                    coa_core::accounts::create(&root, &username, &password, administrator)?;
                    tracing::info!(%username, administrator, "account created");
                    Ok(())
                })
                .await?,
            )
        }
        "account_set_password" => {
            let SetPassword { id, name, password } = args(a)?;
            let root = state.path_of(&id)?;
            answer(
                blocking(move || {
                    Ra::connect(&root)?.set_account_password(&name, &password)?;
                    tracing::info!(%name, "account password changed");
                    Ok(())
                })
                .await?,
            )
        }
        "account_set_access" => {
            let SetAccess { id, name, level } = args(a)?;
            let root = state.path_of(&id)?;
            answer(
                blocking(move || {
                    Ra::connect(&root)?.set_account_access(&name, level)?;
                    tracing::info!(%name, level, "account access level changed");
                    Ok(())
                })
                .await?,
            )
        }
        "get_population" => {
            let PopulationArgs { id, realm } = args(a)?;
            let root = state.path_of(&id)?;
            let population = tokio::task::spawn_blocking(move || {
                let o = process::observe(&root, &layout::read_ports(&root));
                if o.mysql.state != ServiceState::Running {
                    return None;
                }
                match realm {
                    Some(mode) => coa_core::population::query_for_realm(&root, mode).ok(),
                    None => coa_core::population::query(&root).ok(),
                }
            })
            .await
            .unwrap_or(None);
            answer(population)
        }
        "install_environment" => answer(
            blocking(|| {
                Ok(Environment {
                    flavor: coa_core::platform::flavor(),
                    default_dir: default_install_dir(),
                    docker_problem: docker::check_docker(&docker::SystemDocker).err().map(|e| e.to_string()),
                })
            })
            .await?,
        ),
        "default_install_dir" => answer(default_install_dir()),
        "install_preflight" => {
            let PreflightArgs { dest, needed, game_data } = args(a)?;
            let registry = state.registry();
            answer(
                blocking(move || {
                    Ok(docker_install::preflight(
                        Path::new(&dest),
                        Path::new(game_data.as_deref().unwrap_or("")),
                        needed.unwrap_or(1024 * 1024 * 1024),
                        &registry,
                    ))
                })
                .await?,
            )
        }
        "install_requirements" => {
            let RequirementsArgs { package } = args(a)?;
            answer(
                blocking(move || {
                    let (m, _) = coa_core::pkgsource::fetch_manifest(&package_source(package)?, &trusted_key())?;
                    let archive = m
                        .archive
                        .ok_or_else(|| Error::InvalidManifest("no archive".into()))?;
                    Ok(InstallRequirements {
                        download_bytes: archive.parts.iter().map(|p| p.size).sum(),
                        unpacked_bytes: archive.unpacked_size,
                        version: m.version,
                    })
                })
                .await?,
            )
        }
        "install_new" => {
            let InstallArgs { dest, package, game_data } = args(a)?;
            install_new(state.clone(), dest, package, game_data).await
        }
        "cancel_install" => {
            if let Ok(c) = state.install_cancel.lock() {
                if let Some(c) = c.as_ref() {
                    c.cancel();
                }
            }
            answer(())
        }
        "game_data_folder" => {
            let Id { id } = args(a)?;
            let root = state.path_of(&id)?;
            if !docker::is_docker(&root) {
                return answer(None::<String>);
            }
            answer(Some(docker::game_data_dir(&root)?.to_string_lossy().into_owned()))
        }
        "close_to_tray" | "clear_game_cache_on_play" | "browse_enabled" | "manager_installing" => answer(false),
        _ => Err(CallError::Unknown),
    }
}

/// Import: writes only `<folder>.manager` and the registry entry. The server folder is never modified.
async fn add_server(state: Arc<AppState>, path: String) -> Answer {
    let report = blocking({
        let root = PathBuf::from(&path);
        move || layout::scan(&root)
    })
    .await?;
    if report.classification == Classification::Incompatible {
        return Err(Error::Invalid("This folder is not a CoA server folder.".into()).into());
    }
    let registry = state.registry();
    let server = PathBuf::from(&report.path);
    if let Some(existing) = registry.find_by_path(&server)? {
        return answer(summary(existing, server));
    }
    let mut meta = InstallMeta::new(InstallKind::Imported, &server);
    meta.core.commit = report.release.as_ref().and_then(|r| r.main_revision.clone());
    meta.core.version = report.banner_revision.clone();
    meta.database.port = Some(report.ports.mysql);
    meta.database.schemas = report.database_schemas.clone();
    let exe_names = if docker::is_docker(&server) {
        ("Core/worldserver", "Core/authserver")
    } else {
        ("Core/worldserver.exe", "Core/authserver.exe")
    };
    for (rel, exe) in [(exe_names.0, &report.worldserver), (exe_names.1, &report.authserver)] {
        if let Some(e) = exe {
            meta.original_hashes.insert(rel.into(), e.sha256.clone());
        }
    }
    let dir = metadata_dir_for(&server)?;
    let meta = if dir.join("install.json").is_file() {
        MetaDir::open(&dir)?.1
    } else {
        MetaDir::create(&server, &meta)?;
        meta
    };
    registry.register(&meta.id, &server)?;
    tracing::info!(id = %meta.id, path = %server.display(), "server imported (read-only)");
    answer(summary(meta.id, server))
}

/// Start everything. Same preparation as the desktop app (module configs, bind address, performance defaults); the
/// prototype leaves out the companions reconciliation and the Windows firewall rules.
async fn start_server(state: &Arc<AppState>, id: String) -> Answer {
    let root = state.path_of(&id)?;
    let _guard = BusyGuard::acquire(state, &id)?;
    let prep = root.clone();
    let _ = blocking(move || {
        let dir = meta_dir(&prep)?;
        let (_, meta) = MetaDir::open(&dir)?;
        if meta.kind == InstallKind::New {
            coa_core::config::materialize_module_configs(&prep)?;
        } else {
            let _ = coa_core::config::create_missing_module_configs(&prep);
        }
        coa_core::friends::ensure_bind(&prep, &dir)?;
        if meta.kind == InstallKind::New {
            coa_core::config::ensure_performance_defaults(&prep, &dir)?;
        }
        Ok(())
    })
    .await;
    let run = root.clone();
    let out = blocking(move || driver::run(&run, Verb::StartAll)).await?;
    if out.ok {
        let _ = blocking(move || {
            let dir = meta_dir(&root)?;
            coa_core::friends::reapply(&root, &dir)
        })
        .await;
    }
    answer(out)
}

async fn install_new(
    state: Arc<AppState>,
    dest: String,
    package: Option<String>,
    game_data: Option<String>,
) -> Answer {
    let source = package_source(package)?;
    let data_dir = PathBuf::from(
        game_data
            .filter(|d| !d.trim().is_empty())
            .ok_or_else(|| Error::Invalid("Choose the folder that holds the game data.".into()))?,
    );
    let cancel = Cancel::default();
    {
        let mut slot = state
            .install_cancel
            .lock()
            .map_err(|_| Error::Invalid("state poisoned".into()))?;
        if slot.is_some() {
            return Err(Error::Invalid("An installation is already running.".into()).into());
        }
        *slot = Some(cancel.clone());
    }
    let key = trusted_key();
    let events = state.clone();
    let registry = state.registry();
    let result = tokio::task::spawn_blocking(move || {
        docker_install::install(
            &docker_install::Params {
                source,
                dest: PathBuf::from(dest),
                data_dir,
                trusted_key: &key,
                registry: &registry,
                cancel,
            },
            &|step| events.emit("install-progress", step),
        )
    })
    .await;
    if let Ok(mut c) = state.install_cancel.lock() {
        *c = None;
    }
    let done = result.map_err(|e| Error::Invalid(e.to_string()))??;
    answer(summary(done.id, PathBuf::from(done.path)))
}
