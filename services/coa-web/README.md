# coa-web (prototype)

The CoA Server Manager's interface in a browser, managing a **Linux (Docker)** server on the machine where this service
runs. The React interface is unchanged: a small bridge (`src/web/bridge.ts`, included only by `npm run build:web`)
sends Tauri's `invoke(command, args)` to this service over HTTP and receives events over Server-Sent Events.

```
browser ── HTTPS (reverse proxy) ──> coa-web :8787 ──> coa-core ──> docker (mysql, authserver, worldserver)
```

## What works in the prototype

| Area | Commands |
|---|---|
| Servers | `list_servers`, `scan_server`, `add_server`, `forget_server`, `game_data_folder` |
| Start / stop / status | `server_status`, `start_server`, `stop_server`, `force_stop_server` |
| Logs | `console_tail` (world, auth, database) |
| Accounts | `list_accounts`, `create_account`, `account_set_password`, `account_set_access` |
| Players online | `get_population` |
| Install (Docker) | `install_environment`, `default_install_dir`, `install_preflight`, `install_requirements`, `install_new` (progress as `install-progress` events), `cancel_install` |

Every other command answers HTTP 501 with the usual error shape ("not available in the web version yet"), which the
interface shows like any other error. `start_server` leaves out the desktop app's companions reconciliation.
File pickers become a prompt for a path **on the server**.

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
