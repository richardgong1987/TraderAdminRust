# Deployment

abs_admin runs on one server (Alibaba Cloud Hong Kong, `47.76.236.148`) inside **k3s**, a small
Kubernetes. **Ansible**, run from your Mac, sets the server up and deploys new versions. One command
does both, and it skips everything that is already done:

```sh
cd deployment
ansible-playbook site.yml
```

| Document | What it covers |
|---|---|
| This page | What runs where, how to deploy, everyday commands, troubleshooting |
| [how-it-was-built.md](how-it-was-built.md) | **How this was built, step by step**, with the reasons and the mistakes along the way. Read this to learn Ansible and k3s from this project. |
| [migrate-from-compose.md](migrate-from-compose.md) | The one-time move from the old Docker Compose setup on 2026-11-09, and how to undo it |
| [gemma-install.md](gemma-install.md) | Ollama and the Gemma model, by hand |
| [server-specs.md](server-specs.md) | The server's hardware, and how to check it |
| [open-port-8000.md](open-port-8000.md) | How port 8000 was opened to the internet, and how to check any port |

---

## What runs where

```
 Your Mac                               Server 47.76.236.148 (Ubuntu 26.04)
 ────────                               ─────────────────────────────────────────────────────────
 deployment/              SSH           systemd services
 ansible-playbook ──────────────────►   ├─ k3s ─────── namespace abs-admin
   site.yml                             │              ├─ app       Deployment, 1 pod, host network
                                        │              │            listens on :8000 (public)
 GitHub master  ◄── git clone ───────   │              └─ mysql-0   StatefulSet, volume data-mysql-0
 (what gets deployed)                   ├─ ollama      127.0.0.1:11434, gemma4:26b
                                        └─ docker      only builds the app image
```

How the pieces reach each other:

```
 browser ──► :8000  app  ──► 127.0.0.1:11434                       Ollama (server only)
                         └─► mysql.abs-admin.svc.cluster.local:3306 MySQL  (pod network only)
```

- **The app** uses the server's network (`hostNetwork: true`), like the old `network_mode: host`. That is
  how it reaches Ollama on `127.0.0.1`, and why port 8000 is the server's port 8000.
- **MySQL** has no port on the server at all. Only pods reach it, through its Kubernetes Service name.
- **Ollama** is a normal systemd service, not in k3s. It has no login, so it listens on `127.0.0.1` only.

### The files

Everything is in `deployment/`:

| File | What it is |
|---|---|
| `ansible.cfg` | Ansible's settings. Read only when you run Ansible from this folder. |
| `inventory.yml` | The server: its address and the SSH user |
| `site.yml` | The playbook: runs the four roles in order |
| `roles/k3s/` | Installs and configures k3s; creates the namespace and the storage class |
| `roles/ollama/` | Installs and configures Ollama; starts the model download |
| `roles/mysql/` | Creates MySQL's passwords once; runs MySQL |
| `roles/app/` | Builds the app image from GitHub; rolls it out |
| `Dockerfile`, `Dockerfile.dockerignore` | How the app image is built |
| `migrate-from-compose.yml` | The one-time move from Docker Compose. Not needed again. |

On the server, `/opt/abs_admin/k8s/` holds the exact manifests Ansible applied, and `/opt/abs_admin/src/`
the code it built from.

---

## Before the first deploy (on your Mac, once)

1. Ansible: `brew install ansible`. Version 2.21 is installed now (`ansible --version`).
2. SSH login with your key: `ssh root@47.76.236.148 true` must return without asking for a password.
3. Always run Ansible from `deployment/`, or it won't find `ansible.cfg` and the inventory:

   ```sh
   cd deployment
   ansible all -m ping        # should print "pong"
   ```

## Deploying

The server builds what is on **GitHub**, not the files on your Mac. Push first:

```sh
git push
cd deployment
ansible-playbook site.yml
```

What each role does, and when it skips:

| Role | Does | Skips when |
|---|---|---|
| `k3s` | Writes `/etc/rancher/k3s/config.yaml`, installs k3s, starts it, creates namespace `abs-admin` and storage class `local-path-retain` | The installed version is `k3s_version`; the config and manifest match |
| `ollama` | Installs Ollama, sets its context length and keep-alive, starts the model download | Ollama is installed; the override matches; the model is downloaded or downloading |
| `mysql` | Creates secret `mysql` with random passwords, runs MySQL | The secret exists; the running MySQL matches `mysql.yaml` (so **MySQL is not restarted**) |
| `app` | Clones the commit, builds image `abs_admin:<commit>`, loads it into k3s, creates secret `app`, rolls out | The image for this commit exists; the running app matches the manifest |

A run with nothing to do ends with `changed=0`. Useful variations:

| Command | Does |
|---|---|
| `ansible-playbook site.yml --tags app` | Only the app: the usual command after `git push` |
| `ansible-playbook site.yml --check --diff` | Dry run: shows what would change, changes nothing |
| `ansible-playbook site.yml --tags app -e app_version=<commit>` | Deploy one commit. Rolls back when it is an older one. |
| `ansible-playbook site.yml --list-tasks` | Lists every step, without connecting |

Every new commit restarts the app for a few seconds, even a docs-only one: the image is named after the
commit, and a new name is a new version to Kubernetes. Docker's build cache makes such a build take
seconds, because the code didn't change.

The first login after a fresh install is `00000000000` / `123456`. Change the password straight away.

---

## Everyday commands

Run these on the server (`sh ssh.sh` from the repo root). `kubectl` there is k3s's, already logged in as
admin. `-n abs-admin` selects the app's namespace.

| Task | Command |
|---|---|
| Everything at a glance | `kubectl -n abs-admin get pods,svc,pvc` |
| Which commit is running | `kubectl -n abs-admin get deploy app -o jsonpath='{..image}'` |
| App log files | `kubectl -n abs-admin exec deploy/app -- sh -c 'tail -f target/logs/*.log'` |
| Startup errors (before the log files open) | `kubectl -n abs-admin logs deploy/app` |
| Why a pod doesn't start | `kubectl -n abs-admin describe pod -l app=abs-admin`, read *Events* at the bottom |
| Restart the app | `kubectl -n abs-admin rollout restart deploy/app` |
| MySQL shell | `kubectl -n abs-admin exec -it mysql-0 -- sh -c 'mysql -uroot -p"$MYSQL_ROOT_PASSWORD" abs_admin'` |
| Back up the database | `kubectl -n abs-admin exec mysql-0 -- sh -c 'mysqldump -uroot -p"$MYSQL_ROOT_PASSWORD" --single-transaction abs_admin' > backup-$(date +%F).sql` |
| Show the passwords | `kubectl -n abs-admin get secret mysql -o jsonpath='{.data.password}' \| base64 -d` (keys: `root-password`, `password`; secret `app` has `jwt-secret`) |
| Where the data is on disk | `ls /var/lib/rancher/k3s/storage/` |
| Model download progress | `tail -f /root/ollama-pull.log` |
| k3s itself | `systemctl status k3s`, `journalctl -u k3s -n 50` |

The app writes its log to files, not to the console, so `kubectl logs` shows only what happens before
logging starts.

### Save the passwords

The passwords exist only inside k3s. Copy them to a password manager once:

```sh
kubectl -n abs-admin get secret mysql app -o yaml
```

The values there are base64: `echo <value> | base64 -d` decodes one. Why it matters: see Troubleshooting.

### MySQL from your Mac

MySQL has no port on the server, so forward one through k3s and SSH:

```sh
# On the server: forwards the server's 127.0.0.1:3306 to MySQL. Keep it running.
kubectl -n abs-admin port-forward svc/mysql 3306:3306
# On your Mac, in a second terminal. Then connect your client to 127.0.0.1:3306.
ssh -N -L 3306:127.0.0.1:3306 root@47.76.236.148
```

### kubectl from your Mac (optional)

The server's kubeconfig is an admin login for the whole cluster. Keep it out of the repo.

```sh
scp root@47.76.236.148:/etc/rancher/k3s/k3s.yaml ~/.kube/abs-admin.yaml
chmod 600 ~/.kube/abs-admin.yaml
# It already points at https://127.0.0.1:6443, which works through this tunnel:
ssh -N -L 6443:127.0.0.1:6443 root@47.76.236.148
# In another terminal:
KUBECONFIG=~/.kube/abs-admin.yaml kubectl -n abs-admin get pods
```

---

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `Secret mysql is missing, but MySQL already has data` | MySQL reads its passwords on its first start only, so new ones would lock the app out. Recreate the secret with the old values from your password manager: `kubectl -n abs-admin create secret generic mysql --from-literal=root-password=... --from-literal=password=...`. Only if the data doesn't matter: `kubectl -n abs-admin delete statefulset mysql && kubectl -n abs-admin delete pvc data-mysql-0`, then delete the PV and its folder (see Risks, *Retain*). |
| The app rollout times out | Ansible prints the pod list and the app's last log lines. `rbatis pool init fail` means MySQL is unreachable or the password is wrong. `Address already in use` means something else holds port 8000: `ss -ltnp '( sport = :8000 )'`. |
| Pod status `ErrImageNeverPull` | k3s doesn't have the image. Run `ansible-playbook site.yml --tags app`; it rebuilds and loads it. |
| `ansible-playbook` says it can't find the roles or the inventory | You are not in `deployment/`. |
| `--check` stops at `Could not find the requested service k3s` | Only on a server without k3s: there is no service to check yet. Run it for real. |
| Every AI test result is `ERROR` | The model is still downloading (`ollama list` doesn't show it), or Ollama is down (`systemctl status ollama`). |

---

## Risks

| Risk | Mitigation |
|---|---|
| **k3s listens on 6443 (API) and 10250 (kubelet) on every address**, and the security group lets every port in | Both need a client certificate, so nobody gets in without one. Still, close them: in the ECS console, set the security group's inbound rules to 22 and 8000 only. Nothing else needs to be reachable from outside; `kubectl` from your Mac goes through SSH. |
| **One server, data on its local disk** | Back up with the command above, and copy the file off the server. The data folders are under `/var/lib/rancher/k3s/storage/`. |
| **The passwords exist only in k3s** | Save them (above). If secret `mysql` is lost, the deploy stops rather than create new passwords. |
| **Deleting a volume claim** | Storage class `local-path-retain` keeps the folder when its claim is deleted (k3s's default class would delete it). Getting it back is manual: the PV shows `Released`, and its folder is still there. |
| **MySQL's volume settings can't change** | Kubernetes doesn't allow edits to a StatefulSet's `volumeClaimTemplates`. `local-path` doesn't enforce sizes, so it rarely matters; to change it anyway, `kubectl -n abs-admin delete statefulset mysql --cascade=orphan` (the pod and data stay), then deploy again. |
| **Each deploy has a few seconds of downtime** | The old pod stops before the new one starts, because both would need port 8000 (`strategy: Recreate`). Acceptable for one user. |
| **Docker and k3s on one server** | Docker only builds. Both edit the firewall rules; that is a known, working combination, and was checked here (`kubectl get pods -A` all Running). |
| **Unused app images pile up in k3s** | k3s removes unused images on its own when the disk is 85% full. |
| **Plain HTTP on port 8000** | See [open-port-8000.md](open-port-8000.md). |
