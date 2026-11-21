# coa-web (prototype)

The CoA Server Manager's interface in a browser, managing a **Linux (Docker)** server on the machine where this service
runs. The React interface is unchanged: a small bridge (`src/web/bridge.ts`, included only by `npm run build:web`)
sends Tauri's `invoke(command, args)` to this service over HTTP and receives events over Server-Sent Events.

```
browser ── HTTPS (reverse proxy) ──> coa-web :8787 ──> coa-core ──> docker (mysql, authserver, worldserver)
```

## Commands: the desktop Manager's own

The service runs **every command the desktop app registers**, with the desktop's own code. `build.rs` reads
`src-tauri/src`, leaves out what only builds the window (`run()`: window, tray, plugins), points the few `tauri::` paths
the commands use at `src/shim.rs`, and generates the HTTP side of each command from its signature: each argument is read
from the JSON body under the name the interface sends (camelCase, as Tauri does), the result is returned as JSON, an
error as the value the command rejects with. A command added or changed upstream is served as soon as the service
builds again; `GET /api/info` lists them.

What differs from the desktop app:

* Everything runs **on the machine of this service**: backups, logs, game data, client folders, diagnostics. Commands
  about the game client on a player's computer (`play`, `client_*`, `portable_*`) act on this machine too; starting the
  game fails here, as there is no screen.
* Events (`install-progress`, backup progress...) reach every connected browser over Server-Sent Events.
* A link a command opens (the dashboard, a help page) opens in a new tab of the browser; showing a folder does nothing.
* Exiting the Manager or closing it to the tray reloads the page; the service keeps running.
* The Linux package's signing key may be set with `COA_TRUSTED_KEY` (`src/overrides.rs`); the desktop allows that only
  in debug builds.
* File pickers become a prompt for a path **on the server**.

## Build

```bash
npm ci && npm run build:web                                   # interface -> dist-web/
cargo build --release --manifest-path services/coa-web/Cargo.toml
```

## Run

```bash
export COA_WEB_TOKEN=$(openssl rand -hex 32)      # keep it: it is the password of the web interface
export COA_WEB_STATIC=$PWD/dist-web
export COA_PACKAGE_SOURCE=/srv/coa-packages/linux  # folder or https address of a signed Linux package (optional)
services/coa-web/target/release/coa-web            # listens on 127.0.0.1:8787
```

The user running it needs access to Docker (member of the `docker` group). Put an HTTPS reverse proxy in front of it
(for example the Synology one) and do not expose port 8787 directly.

| Variable | Meaning |
|---|---|
| `COA_WEB_TOKEN` | Access token, at least 24 characters (required) |
| `COA_WEB_BIND` | Listen address, default `127.0.0.1:8787` |
| `COA_WEB_STATIC` | Folder of the built interface |
| `COA_WEB_INSECURE_COOKIE=1` | Cookie without `Secure`, only for plain-http tests |
| `COA_WEB_NO_AUTH=1` | No token, refused unless the address is loopback |
| `COA_MANAGER_DATA_DIR` | The Manager's data (`installs.json`), default `~/.local/share/CoAServerManager` |
| `COA_PACKAGE_SOURCE` | Default Linux package (folder or http(s) address) |
| `COA_TRUSTED_KEY` | Public key the package must be signed with, default the official key |

## Run in Docker

```bash
cd services/coa-web/deploy
cp /dev/null .env   # then fill in the values listed at the top of docker-compose.yml
docker compose up -d --build
```

The image holds the interface, the service and the Docker client only; the servers still run as containers of the
host's Docker. Because of that the container needs:

* **the host's Docker socket** (`/var/run/docker.sock`) and the `docker` group (`group_add`);
* **the host network** (`network_mode: host`): the service checks the servers' ports and uses the RA console on
  `127.0.0.1`;
* **the same paths inside and outside**: `/srv/coa` is mounted at `/srv/coa`, and servers and game data go under it,
  because those paths are handed to the host's Docker as bind mounts. The Manager's data is kept in `/srv/coa/.manager`.

Access to the Docker socket is equivalent to root on the host: anyone who logs in to the web interface can, in
effect, control the machine. Keep the token secret and the service behind HTTPS.

## Security

* Login exchanges the token for an `HttpOnly`, `SameSite=Strict` cookie (derived from the token, not the token
  itself); scripts can use `Authorization: Bearer <token>` instead.
* Every `POST` must carry `x-coa-request: 1`, which a page on another site cannot send without a CORS preflight
  (never allowed), so forged requests fail.
* As in the desktop app, commands name installations by id; only `scan_server`, `add_server` and the install
  commands take a path, which the person logged in chooses.

## Tests

```bash
cargo test --manifest-path services/coa-web/Cargo.toml
```

CI: `.github/workflows/web-check.yml` (tests, release build, web build and a smoke test of the running service).
