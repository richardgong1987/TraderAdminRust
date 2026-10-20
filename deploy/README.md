# Docker Deployment

Run abs_admin and its MySQL database on the server with Docker Compose. After a `git pull`, one command
rebuilds and restarts the app, and MySQL keeps running untouched.

- Part 1 is the design: why it is built this way.
- Part 2 is how to use it: first-time setup, deploying updates, and daily operations.

Everything about deployment lives in this `deploy/` folder:

| File | What it is |
|---|---|
| `README.md` | This document |
| `Dockerfile`, `Dockerfile.dockerignore` | How the app image is built |
| `docker-compose.yml` | The app |
| `docker-compose.mysql.yml` | MySQL |
| `.env.example` | Template for `deploy/.env`, which holds the secrets |
| [server-specs.md](server-specs.md) | The server's hardware, and how to check it |
| [gemma-install.md](gemma-install.md) | Installing Ollama and the Gemma model on the server |

---

# Part 1 — Design

## Problem

Deploying by hand on the server is slow and easy to get wrong: install Rust, build, set up MySQL,
configure, restart. We want:

1. **Pull and run.** On the server, `git pull` plus one command gets the new version running.
2. **MySQL stays up.** The database starts once and keeps running. Deploying or restarting the app must
   never restart MySQL. A database restart drops connections and risks the data for no benefit.
3. **No secrets in git.** The repo is public, and `application.json5` holds development defaults
   (`root:password`, JWT secret `abs_admin`). Production must not use them.

## Approach

**Two Compose projects with separate lifecycles, joined by one Docker network.**

```
 docker-compose.mysql.yml            docker-compose.yml
 project "abs_admin_db"              project "abs_admin"
 started once                        rebuilt on every deploy
┌──────────────────────┐            ┌──────────────────────┐
│ abs_admin_mysql      │◄── 3306 ───│ app (abs_admin)      │──► 127.0.0.1:8000
│ mysql:9.7 (LTS)      │            │ Rust binary + dist/  │
│ volume: mysql_data   │            │ volume: app_data     │
└──────────┬───────────┘            └──────────┬───────────┘
           └────── network "abs_admin_net" ────┘
       (created by the DB project; the app project joins it as "external")
```

| File (in `deploy/`) | Purpose |
|---|---|
| `docker-compose.mysql.yml` | MySQL only. Run once with `up -d`. `restart: unless-stopped` brings it back after a crash or server reboot. |
| `docker-compose.yml` | The app only. `docker compose up -d --build` rebuilds and replaces it. It has no way to stop or recreate MySQL. |
| `Dockerfile` | Two stages: compile the release binary, then copy it into a small Debian image with `application.json5` and `dist/`. The build context is the repo root, because the build needs `src/`, `Cargo.toml` and `dist/`. |
| `Dockerfile.dockerignore` | Keeps `target/`, `.git` and secrets out of the build context. BuildKit reads an ignore file named after the Dockerfile, so it can live here instead of the repo root. |
| `.env` (server only, gitignored) | Passwords and the JWT secret. Both Compose files read it from `deploy/`. `.env.example` is the template. |

**How the app finds its secrets.** `src/config/config.rs` still loads `application.json5`. When the
environment variables `DB_URL` and `JWT_SECRET` are set, they replace the file's values at startup. Locally
they are not set, so development works as before. In Docker, `docker-compose.yml` builds both from `.env`.

**How the app waits for MySQL.** It doesn't wait. If MySQL is not reachable at startup, the app exits with
an error, and `restart: unless-stopped` starts it again a few seconds later until MySQL answers. A
`depends_on` cannot work here, because it only applies within one Compose project.

**Faster rebuilds.** The Dockerfile keeps downloaded crates and compiled dependencies in BuildKit cache
mounts, so a deploy recompiles only the code that changed. The final link step is still slow (a few
minutes), because `Cargo.toml` turns on LTO for release builds.

### Rejected alternatives

| Alternative | Why not |
|---|---|
| One Compose file with both services | `docker compose up -d --build` would leave an unchanged MySQL alone, but `docker compose down`, a common reflex when redeploying, would stop the database too. Separate projects make "the app cannot touch MySQL" true by construction. |
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
| **`/admin/download` and `/admin/delete` take any file path and need no login.** Anyone who can reach port 8000 can read files such as `/proc/self/environ` (which holds the DB password and JWT secret) or delete files. | The app is published on `127.0.0.1:8000` only, so it is not reachable from the internet. Use an SSH tunnel. Fix those endpoints before exposing the app publicly (see Part 2). The app also runs as a non-root user, which limits what can be deleted. |
| `.env` is lost or leaked | It exists only on the server. Keep a copy of the values in a password manager. |
| The first build on the server is slow | About 1 GB of images and crates at the server's ~10 Mbps (see [gemma-install.md](gemma-install.md#network-bandwidth)), plus compile time. Later builds reuse the cache. |
| `Cargo.lock` was gitignored, so the server would pick its own dependency versions | `Cargo.lock` is now committed, and the build uses `--locked`. |
| The database driver is chosen at **build** time | `build.rs` picks it from the `db_url` scheme in `application.json5` (currently `mysql`). `DB_URL` can change the host, user and password, but not the database type. |
| Dev uses MySQL 9.1, the server uses 9.7 | Same major line, and the app only uses basic SQL. Tested against 9.7.2 on 2026-10-20. |
| The seeded admin account has a public default password (`00000000000` / `123456`) | Change it in the admin UI right after the first deploy (step 6 below). |
| The app will need Ollama later | Ollama listens on the host's `127.0.0.1`, which a container cannot reach. When the app starts calling Ollama, let Ollama listen on the Docker bridge as well, and connect via `host.docker.internal`. |

---

# Part 2 — How to use it

All commands run on the server, as root, from `/opt/abs_admin/deploy`, unless a step says otherwise.

## First-time setup

### 1. Install Docker

```sh
apt update
apt install -y docker.io docker-compose-v2 docker-buildx
docker --version && docker compose version
```

`docker-buildx` is required, because the Dockerfile uses build cache mounts, which only BuildKit supports.

### 2. Get the code

```sh
cd /opt
git clone https://github.com/richardgong1987/TraderAdminRust.git abs_admin
cd /opt/abs_admin/deploy
```

### 3. Create `.env`

```sh
cp .env.example .env
# Fill in the three values. Generate each one with:
openssl rand -hex 24
nano .env
chmod 600 .env
```

Use only letters and digits in the passwords, because they go into a `mysql://` URL.
`openssl rand -hex` output is safe.

### 4. Start MySQL (once)

```sh
docker compose -f docker-compose.mysql.yml up -d --wait
```

`--wait` returns when MySQL reports healthy. The first start takes 20–60 seconds while it creates the database.

### 5. Start the app

```sh
docker compose up -d --build
```

The first build takes a while (see Risks). Then check it:

```sh
docker compose ps                         # app should be "Up"
curl -s http://127.0.0.1:8000/admin/      # {"code":"0","msg":null,"data":"hello"}
```

### 6. Change the admin password

On first start, the app creates the admin account `00000000000` with password `123456`. Anyone who reads
the source knows these. Open the admin page (see [Opening the admin page](#opening-the-admin-page-from-your-mac)),
log in, and change the password straight away.

## Deploying an update

```sh
cd /opt/abs_admin/deploy
git pull
docker compose up -d --build
```

That's all. MySQL is not touched. Old images pile up over time; clean them occasionally with
`docker image prune -f`.

## Opening the admin page from your Mac

The app only listens on the server's `127.0.0.1`. Forward it over SSH:

```sh
# On your Mac. Keep it running; Ctrl+C closes it.
ssh -N -L 8000:127.0.0.1:8000 root@47.76.236.148
```

Then open http://localhost:8000. To reach MySQL with a desktop client, forward `3306` the same way.

## Everyday commands

| Task | Command |
|---|---|
| Status of everything | `docker ps` |
| App logs | `docker compose exec app sh -c 'tail -f target/logs/*.log'` |
| Restart the app only | `docker compose restart app` |
| Stop the app (MySQL keeps running) | `docker compose down` |
| MySQL shell | `docker exec -it abs_admin_mysql mysql -uroot -p` |
| Back up the database | `docker exec abs_admin_mysql sh -c 'mysqldump -uroot -p"$MYSQL_ROOT_PASSWORD" abs_admin' > backup-$(date +%F).sql` |
| Restart MySQL (rarely needed) | `docker compose -f docker-compose.mysql.yml restart` |

The app writes logs to files, not to `docker logs`. In release builds the app turns console logging off.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `network abs_admin_net declared as external, but could not be found` | MySQL's project has not been started. Run step 4. |
| `required variable MYSQL_PASSWORD is missing` | `.env` is missing or incomplete. Run step 3. |
| App keeps restarting | Read its logs (above). `rbatis pool init fail` means MySQL is not reachable or the password in `.env` changed after MySQL's first start. MySQL only reads the passwords on its **first** start, so changing them in `.env` later has no effect on MySQL. |
| `the --mount option requires BuildKit` | Install `docker-buildx` (step 1). |

## Exposing the app publicly (not yet)

Before opening port 8000 (or 80/443) in the security group, fix `/admin/download` and `/admin/delete`. They
must require a login and only accept paths inside `target/upload/`. After that, change `127.0.0.1:8000:8000`
in `docker-compose.yml` to `8000:8000`, ideally behind a reverse proxy with HTTPS.
