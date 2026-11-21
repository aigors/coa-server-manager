# Running a CoA server on a remote Linux machine with coa-web

coa-web runs **on the server** and installs the game server **on the same machine**, in Docker. You only open a browser
on your own computer:

```
your computer (browser) ── HTTPS ──> reverse proxy ──> coa-web 127.0.0.1:8787
                                                         └── host Docker: MySQL, authserver, worldserver
players (game client) ─────────────> ports 3724 (login) and 8085 (world)
```

Everything below happens once, except step 8 (opening the server to players), which is a choice in the interface.

## 0. What you need

* A Linux x86_64 machine you control (VPS or your own): Ubuntu 24.04 or Debian 12, 4+ cores, 8 GB RAM (16 GB with many
  bots), about 30 GB free disk (package 3 GB, server 8.3 GB, game data 7 GB, databases and backups).
* A DNS name pointing to it (for HTTPS), for example `coa.example.com`.
* On your Windows computer: the **CoA game client** (the folder with `Data\common.MPQ`), to extract its DBC tables.
* A Linux server package: the `linux-unsigned-<version>` pre-release of your build fork (`build-linux.yml`).

## 1. Prepare the machine

```bash
curl -fsSL https://get.docker.com | sudo sh
sudo apt-get install -y git zstd python3-cryptography
sudo mkdir -p /srv/coa && sudo chown "$USER:$USER" /srv/coa
```

Everything the Manager handles lives under `/srv/coa`: coa-web gets that folder at the **same path** inside its
container, because it hands these paths to the host's Docker.

## 2. Get the package and sign it

The fork publishes the package unsigned. The Manager installs only a package whose `manifest.json` is signed by a key it
trusts, so sign it with your own key (keep the private key off the server if you can: signing can be done anywhere).

```bash
V=0.261119.6                                  # the version of your linux-unsigned-<version> release
B=https://github.com/<your-account>/coa-server-build/releases/download/linux-unsigned-$V
mkdir -p /srv/coa/packages/$V && cd /srv/coa/packages/$V
for f in manifest.json release-lock.json compatibility-matrix.json qualification.json base.tar.zst.001 base.tar.zst.002; do
  curl -fLO "$B/$f"
done
sha256sum base.tar.zst.*                      # compare with the "parts" listed in qualification.json

git clone -b web-backend https://github.com/<your-account>/coa-server-manager.git ~/coa-server-manager
D=~/coa-server-manager/services/coa-web/deploy
python3 $D/sign-package.py keygen ~/coa-signing-key      # once; prints COA_TRUSTED_KEY=...
python3 $D/sign-package.py sign ~/coa-signing-key /srv/coa/packages/$V
```

Every new package needs `sign` again. Losing the key means signing new packages with a new one and changing
`COA_TRUSTED_KEY`.

## 3. Game data

The world server reads `dbc`, `maps`, `vmaps` and `mmaps` from one folder, mounted read-only. The maps come with the
package; the DBC tables must come from the **CoA client** (an older or partial set makes the world server drop content
or stop).

**Maps from the package** (on the server):

```bash
mkdir -p /srv/coa/gamedata.tmp
cat /srv/coa/packages/$V/base.tar.zst.* | zstd -dc | tar -x -C /srv/coa/gamedata.tmp --wildcards 'Data/*'
mv /srv/coa/gamedata.tmp/Data /srv/coa/gamedata && rmdir /srv/coa/gamedata.tmp
```

**DBC from the client** (on your Windows computer, with Python 3.11+):

1. Download `mpqcli-windows-x86_64.exe` from <https://github.com/TheGrayDot/mpqcli/releases> and check its SHA-256
   against the release page.
2. Get the server fork's sources at the commit of your package (`core` in `release-lock.json`), for the extractor:
   `git clone https://github.com/Corfirean/azerothcore-wotlk-coa core && git -C core checkout <core commit>`.
3. Extract and check:

```powershell
python extract-client-dbc.py core "C:\path\to\client\Data" dbc-out mpqcli-windows-x86_64.exe
python core\apps\coa-dbc\client_dbc.py check dbc-out
```

4. Copy the tables to the server, as both the server's set and the client's unpatched copy:

```powershell
scp dbc-out\*.dbc you@coa.example.com:/srv/coa/gamedata/dbc/
ssh you@coa.example.com "mkdir -p /srv/coa/gamedata/dbc_clientset && cp /srv/coa/gamedata/dbc/*.dbc /srv/coa/gamedata/dbc_clientset/"
```

The folder should now hold `dbc` (about 370 tables), `dbc_clientset`, `maps`, `vmaps`, `mmaps` and `dbc_races`.

## 4. Start coa-web

```bash
cd ~/coa-server-manager/services/coa-web/deploy
cat > .env <<EOF
COA_WEB_TOKEN=$(openssl rand -hex 32)
COA_UID=$(id -u)
COA_GID=$(id -g)
DOCKER_GID=$(getent group docker | cut -d: -f3)
COA_WEB_BIND=127.0.0.1:8787
COA_PACKAGE_SOURCE=/srv/coa/packages/$V
COA_TRUSTED_KEY=$(cat ~/coa-signing-key/public.b64)
EOF
chmod 600 .env
docker compose up -d --build        # first build: 15-25 minutes and about 4 GB of RAM (interface + Rust)
docker compose logs -f              # "coa-web starting"
grep COA_WEB_TOKEN .env             # the password of the web interface
```

On a small VPS, build the image on another Linux machine instead (`docker build -f services/coa-web/Dockerfile -t
coa-web:local .` from the repository root, then `docker save coa-web:local | ssh server docker load`) and start it with
`docker compose up -d` (no `--build`).

## 5. HTTPS in front of it

coa-web listens only on `127.0.0.1`. Put a reverse proxy with a certificate in front of it. With Caddy (automatic
certificates):

```bash
sudo apt-get install -y caddy
echo 'coa.example.com {
    reverse_proxy 127.0.0.1:8787
}' | sudo tee /etc/caddy/Caddyfile
sudo systemctl reload caddy
```

With nginx, also turn buffering off for the event stream, or progress bars freeze:

```nginx
location /api/events { proxy_pass http://127.0.0.1:8787; proxy_buffering off; proxy_read_timeout 1h; }
location /           { proxy_pass http://127.0.0.1:8787; }
```

Open only 80 and 443 for the interface (`sudo ufw allow 80,443/tcp`). The session cookie is marked `Secure`, so the
interface works only over HTTPS (or plain `http://localhost` through an SSH tunnel:
`ssh -L 8787:127.0.0.1:8787 you@coa.example.com`, with `COA_WEB_INSECURE_COOKIE=1`).

## 6. Install the server from the browser

Open `https://coa.example.com`, enter the token, then **Host a server → Install new server**:

* install folder: `/srv/coa/CoaServer` (anything under `/srv/coa`);
* game data folder: `/srv/coa/gamedata`;
* package: leave it empty (coa-web uses `COA_PACKAGE_SOURCE`).

Folder pickers are a text field for a path **on the server**. The install takes a few minutes; then start the server
from the Overview.

## 7. Accounts

**Accounts → Create account**: your own account with administrator access, then one per player (or let players use
whatever registration you set up).

## 8. Let players in

1. Open the game ports on the machine and in the provider's firewall, if it has one:
   `sudo ufw allow 3724,8085/tcp`. Do **not** open 3306 or 3443: the database is never published and the remote
   console stays on 127.0.0.1.
2. In the interface, **Play with Friends → Over the internet** (check the connection first): it finds the public address of the server, publishes
   the login and world ports beyond the machine, allows remote CoA clients and writes the address into the realm list.
   Restart the server when asked.
3. Players set their client's realmlist to the server: `set realmlist coa.example.com` (or its IP).

## Day to day

* **Backups**: Backups page (quick, full, database only). They are files under the server folder: copy them off the
  machine too.
* **Updates**: there is no published Linux update channel yet; "Check for updates" answers that the package is for
  Windows. A new Linux version means a new package, signed (step 2), and a new install or restore.
* **Logs**: Console page, or `docker logs coa-<project>-world` on the machine.
* **coa-web itself**: `git pull && docker compose up -d --build` in `services/coa-web/deploy`.

## Security

* Access to the Docker socket is access to the whole machine: whoever has the token can control it. Keep the token
  secret, long and random; change it in `.env` and restart to log everyone out.
* Keep coa-web behind HTTPS. Do not publish 8787 directly.
* Everything the interface does runs on this machine: client-side features (`Play`, game client download, portable
  characters) act on the server, not on the visitor's computer.
