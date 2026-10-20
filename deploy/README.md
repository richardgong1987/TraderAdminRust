# Docker Deployment

Run abs_admin, its MySQL database and Ollama on the server. After a `git pull`, one script brings
everything up: it installs what is missing, rebuilds and restarts the app, and leaves MySQL running.

- Part 1 is the design: why it is built this way.
- Part 2 is how to use it: first-time setup, deploying updates, and daily operations.

Everything about deployment lives in this `deploy/` folder:

| File | What it is |
|---|---|
| `README.md` | This document |
| `deploy.sh` | The one command for the first deploy and every update |
| `Dockerfile`, `Dockerfile.dockerignore` | How the app image is built |
| `docker-compose.yml` | The app |
| `docker-compose.mysql.yml` | MySQL |
| `.env.example` | What `deploy/.env` holds: the secrets. `deploy.sh` creates `.env` on the first run. |
| [server-specs.md](server-specs.md) | The server's hardware, and how to check it |
| [gemma-install.md](gemma-install.md) | Installing Ollama and the Gemma model on the server, step by step |

---

# Part 1 — Design

## Problem

Deploying by hand on the server is slow and easy to get wrong: install Docker and Ollama, download the
model, create passwords, set up MySQL, build, restart. We want:

1. **Pull and run.** On the server, `git pull` plus one command gets the new version running. The same
   command does the first deploy on a fresh server.
2. **MySQL stays up.** The database starts once and keeps running. Deploying or restarting the app must
   never restart MySQL. A database restart drops connections and risks the data for no benefit.
3. **No secrets in git.** The repo is public, and `application.json5` holds development defaults
   (`root:password`, JWT secret `abs_admin`). Production must not use them.
4. **Nothing public by accident.** Ollama has no login, MySQL needs none from localhost, and the app has
   no HTTPS yet. All three listen on the server's `127.0.0.1` only.

## Approach

**Three services, each with its own lifecycle. All of them listen on the server's `127.0.0.1` only.**

```
             the server's 127.0.0.1 (nothing listens on the public IP)

   :3306 ◄──────────────── :8000 ────────────────► :11434
   MySQL 9.7 (LTS)         app (abs_admin)         Ollama + gemma4:26b
   container               container on the        systemd service
   abs_admin_mysql         host's network

   docker-compose          docker-compose          gemma-install.md
   .mysql.yml              .yml
   started once            rebuilt on every        installed once
                           deploy
```

| File (in `deploy/`) | Purpose |
|---|---|
| `deploy.sh` | Runs every step below in order. Each step skips what is already done, so the first deploy and every update are the same command. |
| `docker-compose.mysql.yml` | MySQL only, as Compose project `abs_admin_db`. `up -d` leaves a running MySQL alone. `restart: unless-stopped` brings it back after a crash or server reboot. |
| `docker-compose.yml` | The app only, as project `abs_admin`. `docker compose up -d --build` rebuilds and replaces it. It has no way to stop or recreate MySQL. |
| `Dockerfile` | Two stages: compile the release binary, then copy it into a small Debian image with `application.json5` and `dist/`. The build context is the repo root, because the build needs `src/`, `Cargo.toml` and `dist/`. |
| `Dockerfile.dockerignore` | Keeps `target/`, `.git` and secrets out of the build context. BuildKit reads an ignore file named after the Dockerfile, so it can live here instead of the repo root. |
| `.env` (server only, gitignored) | Passwords and the JWT secret. `deploy.sh` creates it with random values on the first run. Both Compose files read it from `deploy/`. |

**How the app reaches MySQL and Ollama.** The app container uses the host's network
(`network_mode: host`), so `127.0.0.1` inside it is the server's `127.0.0.1`. It reaches Ollama at
`127.0.0.1:11434` (the default in `application.json5`) and MySQL at `127.0.0.1:3306`, where the MySQL
container's port is published. A container on a normal Docker network could not reach either one.

**How the app gets its settings.** `src/config/config.rs` still loads `application.json5`. When the
environment variables `SERVER_URL`, `DB_URL` and `JWT_SECRET` are set, they replace the file's values at
startup. Locally they are not set, so development works as before. `docker-compose.yml` sets
`SERVER_URL=127.0.0.1:8000`, because the file's `0.0.0.0:8000` would listen on the public IP once the app
shares the host's network, and builds `DB_URL` and `JWT_SECRET` from `.env`.

**How the app waits for MySQL.** `deploy.sh` starts MySQL with `--wait`, which returns once MySQL reports
healthy, and only then starts the app. If MySQL is down later, the app exits with an error at startup, and
`restart: unless-stopped` starts it again a few seconds later until MySQL answers. A `depends_on` cannot
work here, because it only applies within one Compose project.

**The model downloads in the background.** It takes hours at this server's bandwidth. `deploy.sh` starts
the download and goes on, so the admin pages work at once. AI test runs end in `ERROR` until it finishes.
The model name comes from `ollama_model` in `application.json5`, the same setting the app uses.

**Faster rebuilds.** The Dockerfile keeps downloaded crates and compiled dependencies in BuildKit cache
mounts, so a deploy recompiles only the code that changed. The final link step is still slow (a few
minutes), because `Cargo.toml` turns on LTO for release builds.

### Rejected alternatives

| Alternative | Why not |
|---|---|
| One Compose file with both services | `docker compose up -d --build` would leave an unchanged MySQL alone, but `docker compose down`, a common reflex when redeploying, would stop the database too. Separate projects make "the app cannot touch MySQL" true by construction. |
| The app on a Docker network, reaching Ollama through `host.docker.internal` | Ollama would have to listen on an address besides `127.0.0.1`. It has no login, so a single firewall mistake would let anyone use it. With host networking, all three services stay on `127.0.0.1`. |
| `deploy.sh` runs `git pull` itself | You pull, look at what changed, then deploy. The script deploys exactly what is checked out. |
| MySQL installed directly on the server | Works, but this way the whole stack is described in the repo, pinned to a version, and moves to a new server unchanged. |
| MySQL 26.x, the newest Innovation release | Since July 2026, MySQL numbers releases by date (26.7, 26.10, ...). Each Innovation release is supported only until the next one, so staying supported means upgrading the database every ~3 months, and upgrades can't be undone. **9.7 is the newest LTS.** It has every 9.x feature and gets fixes for years. Switch to Innovation only when a feature you need exists only there: change `mysql:9.7` to that version in `docker-compose.mysql.yml`. |
| Ollama in Docker too | Ollama is already installed natively ([gemma-install.md](gemma-install.md)), and Docker adds nothing for a CPU-only setup. |

## Data

No schema changes. On startup the app creates its tables and seeds the admin user, as before.

| Volume | Mounted at | Holds | Removed by |
|---|---|---|---|
| `abs_admin_db_mysql_data` | `/var/lib/mysql` | All database data | Only `docker compose -f docker-compose.mysql.yml down -v`. **Never run that on the server.** |
| `abs_admin_app_data` | `/app/target` | Uploaded files (`target/upload/`) and logs (`target/logs/`) | `docker compose down -v` |

The app connects as a MySQL user named `abs_admin`, which only has rights on the `abs_admin` database.
The MySQL image creates that user the first time it starts.

## Risks

| Risk | Mitigation |
|---|---|
| **`.env` is lost or leaked** | It exists only on the server. Copy it to a password manager after the first deploy. MySQL only reads the passwords on its first start, so if `.env` is lost, new passwords would lock the app out. `deploy.sh` refuses to create a new `.env` when MySQL already has data. |
| **Uploaded files can be downloaded without a login.** The admin UI shows uploads with `<img src>`, which cannot send a login token, so `/admin/download` has to stay public. | Fixed on 2026-10-20: download only serves files under `target/upload/`. Delete needs a login and only works on your own folder. Upload rejects names like `../../x`. Until downloads need a login, **don't upload private files** through `/admin/upload`. AI training images don't use it; they are stored in MySQL. The app also runs as a non-root user. |
| The app shares the host's network | Anything on `127.0.0.1` is reachable from the app, which is the point: MySQL and Ollama. The app itself listens on `127.0.0.1:8000` only, through `SERVER_URL`. |
| Host networking works this way on Linux only | On Docker Desktop for Mac, `network_mode: host` is off by default, and when it's on, `127.0.0.1` means Docker's Linux VM, not the Mac. Develop locally with `cargo run`, not with this Compose file. |
| The first deploy is slow | About 1 GB of images and crates at the server's ~10 Mbps (see [gemma-install.md](gemma-install.md#network-bandwidth)), plus compile time: roughly half an hour. The model takes hours more, in the background. Later deploys reuse the cache. |
| `Cargo.lock` was gitignored, so the server would pick its own dependency versions | `Cargo.lock` is now committed, and the build uses `--locked`. |
| The database driver is chosen at **build** time | `build.rs` picks it from the `db_url` scheme in `application.json5` (currently `mysql`). `DB_URL` can change the host, user and password, but not the database type. |
| Dev uses MySQL 9.1, the server uses 9.7 | Same major line, and the app only uses basic SQL. Tested against 9.7.2 on 2026-10-20. |
| The seeded admin account has a public default password (`00000000000` / `123456`) | Change it in the admin UI right after the first deploy (step 4 below). |
| Ports 3306 or 8000 are already taken on the server | `deploy.sh` stops with Docker's "address already in use" error. For MySQL, set `MYSQL_HOST_PORT` in `.env`. |

---

# Part 2 — How to use it

Commands run on the server, as root. The examples use `/home/app` for the code; any folder works.

## First-time setup

### 1. Get the code

```sh
git clone https://github.com/richardgong1987/TraderAdminRust.git /home/app
```

### 2. Run the deploy script

```sh
bash /home/app/deploy/deploy.sh
```

It prints each step as it goes, and ends with how to open the admin page. The first run takes roughly half
an hour (see Risks). What it does, in order, and how to do each step by hand:

| Step | Skipped when | By hand |
|---|---|---|
| 1. Install Docker | Docker with Compose and buildx is installed | `apt install -y docker.io docker-compose-v2 docker-buildx`. buildx is required, because the Dockerfile's build cache mounts need BuildKit. |
| 2. Install and configure Ollama | `ollama` is installed; the override file exists | [gemma-install.md](gemma-install.md), Steps 4 and 5 |
| 3. Download the model, in the background | It is downloaded or downloading | [gemma-install.md](gemma-install.md), Step 6 |
| 4. Create `deploy/.env` with random passwords | `.env` exists | `cp .env.example .env`, fill in each value with `openssl rand -hex 24`, `chmod 600 .env` |
| 5. Start MySQL and wait until it's healthy | It's running, and `docker-compose.mysql.yml` is unchanged | `docker compose -f docker-compose.mysql.yml up -d --wait` |
| 6. Build and restart the app, and wait until it answers | Never: every run rebuilds it | `docker compose up -d --build`, then `curl -s http://127.0.0.1:8000/admin/` should print `{"code":"0","msg":null,"data":"hello"}` |

Steps 4 to 6 run from `/home/app/deploy`. The script also removes the old app image a rebuild leaves behind.

### 3. Save the secrets

```sh
cat /home/app/deploy/.env
```

Copy the values to a password manager. See Risks for why.

### 4. Change the admin password

On first start, the app creates the admin account `00000000000` with password `123456`. Anyone who reads
the source knows these. Open the admin page (see [Opening the admin page](#opening-the-admin-page-from-your-mac)),
log in, and change the password straight away.

## Deploying an update

```sh
cd /home/app
git pull
bash deploy/deploy.sh
```

That's all. MySQL is not touched, and steps 1 to 5 finish in seconds because everything is already there.

## Opening the admin page from your Mac

The app only listens on the server's `127.0.0.1`. Forward it over SSH:

```sh
# On your Mac. Keep it running; Ctrl+C closes it.
ssh -N -L 8000:127.0.0.1:8000 root@47.76.236.148
```

Then open http://localhost:8000. To reach MySQL with a desktop client, forward `3306` the same way.

## Everyday commands

Run these from `/home/app/deploy`.

| Task | Command |
|---|---|
| Status of everything | `docker ps` and `systemctl status ollama --no-pager` |
| App logs | `docker compose exec app sh -c 'tail -f target/logs/*.log'` |
| Startup errors (before the log files open) | `docker compose logs app` |
| Restart the app only | `docker compose restart app` |
| Stop the app (MySQL keeps running) | `docker compose down` |
| MySQL shell | `docker exec -it abs_admin_mysql mysql -uroot -p` |
| Back up the database | `docker exec abs_admin_mysql sh -c 'mysqldump -uroot -p"$MYSQL_ROOT_PASSWORD" abs_admin' > backup-$(date +%F).sql` |
| Restart MySQL (rarely needed) | `docker compose -f docker-compose.mysql.yml restart` |
| Model download progress | `tail -f /root/pull.log` |

The app writes logs to files, not to `docker logs`. In release builds the app turns console logging off.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `.env is missing, but MySQL already has data` | `deploy.sh` won't create new passwords, because MySQL still expects the old ones. Put the old `.env` back. Only if the data doesn't matter: `docker compose -f docker-compose.mysql.yml down -v` **deletes the whole database**, then run `deploy.sh` again. |
| `The app is not answering at http://127.0.0.1:8000` | The script prints the app's last log lines. `rbatis pool init fail` means MySQL is not reachable, or the password in `.env` changed after MySQL's first start. MySQL only reads the passwords on its **first** start, so changing them in `.env` later has no effect on MySQL. |
| `address already in use` | Another program uses port 8000 or 3306. Find it with `ss -tlnp`. For MySQL, set `MYSQL_HOST_PORT` in `.env`. |
| `required variable MYSQL_PASSWORD is missing` | Running Compose by hand without `.env`. Run `deploy.sh` once, or create `.env` (step 4 of the table). |
| `the --mount option requires BuildKit` | Install `docker-buildx`. |
| Every AI test result is `ERROR` | The model is still downloading (`ollama list` doesn't show it yet), or Ollama is down (`systemctl status ollama`). |

## Exposing the app publicly (not yet)

The app listens only on `127.0.0.1:8000`. Before opening it to the internet:

1. Change the admin password (step 4).
2. Put a reverse proxy with HTTPS in front of it, such as Caddy or nginx, on the server. It forwards to
   `127.0.0.1:8000`, so the app's settings don't change. Without HTTPS, passwords and login tokens cross
   the internet in plain text.
3. Open 443 in the security group.

The file endpoints were fixed on 2026-10-20 (see Risks), so they no longer block this.
