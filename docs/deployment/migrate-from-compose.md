# Moving from Docker Compose to k3s (2026-11-09)

Until 2026-11-09 the server ran abs_admin with Docker Compose (the `deploy/` folder, removed in commit
`5bd761c`). `deployment/migrate-from-compose.yml` moved it to k3s once. This page records what happened,
how to undo it, and what to clean up afterwards. How the playbook works: Step 7 of
[how-it-was-built.md](how-it-was-built.md).

## What happened

```sh
cd deployment
ansible-playbook migrate-from-compose.yml
```

| Step | Old stack | Result |
|---|---|---|
| 1. k3s, Ollama, MySQL in k3s | Serving | Done earlier the same morning, so all `ok` |
| 2. Build `abs_admin:5bd761ce61be` | Serving | 75 s, from Docker's existing build cache |
| 3. Stop the old app, dump its database to `/root/abs_admin-before-k3s.sql`, load the dump into k3s's MySQL, stop the old MySQL | **Down** | 1 user, 2 model versions, 1 example, 1 image moved |
| 4. Start the app in k3s | Down until ready | Ready at 01:38:35 UTC |
| 5. Copy `target/upload` into the app's new volume | — | 2 files |

Downtime was about 15 seconds. The run took 2 min 36 s.

What is still on the server from the old stack:

| What | Where | Why it was kept |
|---|---|---|
| Old volumes `abs_admin_db_mysql_data` (MySQL data) and `abs_admin_app_data` (uploads, logs) | `docker volume ls` | To roll back |
| Old images `abs_admin:latest`, `mysql:9.7` (1.4 GB) | `docker images` | To roll back without a rebuild |
| The dump | `/root/abs_admin-before-k3s.sql` | A backup of the data as it was at the switch |
| The old checkout, with the old passwords in `deploy/.env` | `/home/TraderAdminRust` | It holds the Compose files the rollback needs |

## Rolling back to Docker Compose

Only if k3s causes a problem you can't fix. On the server:

```sh
# 1. Stop the app in k3s, which frees port 8000. MySQL in k3s keeps its data.
kubectl -n abs-admin scale deployment app --replicas=0

# 2. Start the old stack, exactly as it was.
cd /home/TraderAdminRust/deploy
docker compose -f docker-compose.mysql.yml up -d --wait
docker compose up -d
curl -s http://127.0.0.1:8000/admin/
```

Anything written after 01:38 UTC on 2026-11-09 is only in k3s's MySQL. To carry it back, dump it from
k3s (README.md, "Back up the database") before step 2, and load it into the old MySQL after.

While rolled back, **don't run `ansible-playbook`**: it would set the app back to one replica, and the
new pod would fight the old app for port 8000.

To go forward to k3s again: `docker compose down` in that folder (for both Compose files), then
`ansible-playbook site.yml`.

## Cleaning up, once you trust the new setup

After a week or two without problems, on the server:

```sh
docker volume rm abs_admin_app_data abs_admin_db_mysql_data   # the old data: no rollback after this
docker rmi abs_admin:latest mysql:9.7
rm -rf /home/TraderAdminRust                                  # the old checkout and the old passwords
```

Copy `/root/abs_admin-before-k3s.sql` off the server if you want to keep it, then delete it. Keep
Docker itself: it builds the app image (`roles/app/tasks/build.yml`), and its build cache is what makes
rebuilds fast.
