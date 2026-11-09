# How the k3s Deployment Was Built, Step by Step

This is the story of `deployment/`, in the order it was built, so you can learn Ansible and k3s from a
real project. Each step says what was written, why, how it was checked, and what went wrong. The real
command output is included where it teaches something.

The request was:

1. Ansible manages the server.
2. Ansible installs k3s, and skips that when it is already installed.
3. Ansible configures what's needed, and skips what is already configured.
4. Ansible starts the services, but the database must not be restarted on every deploy.

[README.md](README.md) explains how to use the result. This page explains how it came about.

---

## Part 0 — The ideas you need

### Ansible in five minutes

Ansible runs on your Mac (the **control node**) and logs in to servers (**managed nodes**) over plain
SSH. Nothing is installed on the server except Python, which Ubuntu already has.

| Word | Meaning | In this project |
|---|---|---|
| **Inventory** | The list of servers, and how to log in | `inventory.yml`: host `hk`, `root@47.76.236.148` |
| **Module** | One kind of action, written to be safe to repeat | `apt` (packages), `copy` (files), `systemd_service`, `git`, `command` |
| **Task** | One call of a module, with a name | `- name: Start k3s` + `ansible.builtin.systemd_service: ...` |
| **Playbook** | Which hosts get which tasks | `site.yml` |
| **Role** | A folder of tasks and their files, for one component | `roles/k3s`, `roles/ollama`, `roles/mysql`, `roles/app` |
| **Handler** | A task that runs only when another task changed something | "Restart k3s" runs only when the k3s config file changed |
| **Tag** | A label to run part of a playbook | `--tags app` |
| **Variable** | A value tasks use. Role defaults live in `roles/<role>/defaults/main.yml`, and `-e name=value` overrides them. | `k3s_version`, `app_version` |
| **register / when** | Save a task's result, then decide on it | `register: k3s_installed`, then `when: k3s_version not in k3s_installed.stdout` |

The idea that matters most is **idempotency**: you describe the end state, not the steps, and running it
twice is the same as running it once. Each task reports one of:

- `ok`: already in that state, nothing done.
- `changed`: it did something.
- `skipping`: its `when:` condition was false.

That is exactly the "skip what is already done" of the request. A second run of a correct playbook ends
with `changed=0`, and that is the main test used below.

Modules like `apt`, `copy` and `systemd_service` are idempotent by themselves. `command` and `shell` are
not: Ansible can't know what a command does. For those you add the idempotency yourself, with
`creates:`, with `when:`, or with `changed_when:`. Most of the work in this project is exactly that.

### Kubernetes and k3s in five minutes

Kubernetes runs containers from **manifests**: YAML files that describe what should exist. You hand them
to `kubectl apply`, and Kubernetes keeps making reality match them, including after a crash or a reboot.
It is idempotent the same way Ansible is: apply the same file twice, nothing happens the second time.

| Object | What it is | Here |
|---|---|---|
| **Node** | A machine in the cluster | The one server |
| **Pod** | One or more containers that run together | `app-…`, `mysql-0` |
| **Deployment** | Keeps N identical pods running; replaces them when the manifest changes | The app |
| **StatefulSet** | Like a Deployment, for things with data: stable names (`mysql-0`), one volume per pod for life | MySQL |
| **Service** | A stable name for a set of pods | `mysql` → `mysql.abs-admin.svc.cluster.local` |
| **PersistentVolumeClaim (PVC)** | "I need a disk"; the **StorageClass** decides where it comes from | `data-mysql-0`, `app-data` |
| **Secret** | Passwords, given to pods as environment variables | `mysql`, `app` |
| **Namespace** | A folder for objects | `abs-admin` |

**k3s** is a complete Kubernetes in one ~70 MB binary. It includes what a plain Kubernetes would make you
install separately: a container runtime (containerd), pod networking (flannel), DNS (CoreDNS), local disk
volumes (local-path-provisioner), and a few extras we switch off (Traefik, ServiceLB). It installs with
one script and runs as one systemd service, `k3s`.

---

## Part 1 — Look before you build

Before writing anything, I looked at the server. Every command here only reads:

```sh
ssh root@47.76.236.148 'which k3s docker ollama; docker ps; docker volume ls; ollama list; ss -ltnp'
```

What it showed, and why each point shaped the design:

| Found | Consequence |
|---|---|
| The old Docker Compose stack was live: app and MySQL, up two weeks | Real data to move: 1 user, 2 model versions, 1 example, uploaded files. A one-time migration is needed, and it must keep a way back. |
| Ollama ran as a systemd service on `127.0.0.1:11434`, with the 18 GB model downloaded | Leave Ollama where it is. Moving it into k3s gains nothing on a CPU-only server. |
| Docker was installed | Use it to build the app image. No new build tool needed. |
| The security group lets every port in ([open-port-8000.md](open-port-8000.md)) | Every new listening port is public. Avoid any that have no login. |
| `/home/TraderAdminRust` was the old checkout, with a local edit and the old `.env` | Don't reuse it. Ansible gets its own checkout, `/opt/abs_admin/src`. |
| No k3s, Ubuntu 26.04, x86_64, Python 3.14 | Ansible works with no extra setup. |

### The decisions

| Question | Decision | Why |
|---|---|---|
| How does the app reach Ollama, which has no login and must stay on `127.0.0.1`? | The app pod uses the server's network: `hostNetwork: true` | A normal pod has its own network, where `127.0.0.1` is the pod itself. With host networking, `127.0.0.1` is the server, the same trick as the old `network_mode: host`. The alternative, making Ollama listen on an address pods can reach, would expose a service without a login. |
| Then how is the app reachable from outside? | It listens on the server's port 8000 directly | That follows from host networking. So Traefik (the Ingress) and ServiceLB are not needed, and are switched off. |
| How does the app reach MySQL? | Through a Service: `mysql.abs-admin.svc.cluster.local` | The Kubernetes way. MySQL gets no port on the server at all, which is safer than the old `127.0.0.1:3306`. The app needs `dnsPolicy: ClusterFirstWithHostNet` to resolve that name, because host-network pods use the server's DNS by default. |
| Deployment or StatefulSet for MySQL? | StatefulSet | It gives MySQL a stable name and a volume that belongs to `mysql-0` for life. |
| "Don't restart the DB on every deploy"? | Apply MySQL's manifest, but only when it differs from what runs | Kubernetes restarts a pod only when its manifest changes. Ansible runs `kubectl diff` first and skips `kubectl apply` when there is no difference (Step 5). |
| Where is the app image built? | On the server, with Docker, from a clone of GitHub `master` | The Mac is ARM; building an x86 Rust release image there would mean emulation, which is very slow. A registry (GitHub Container Registry, built by CI) is the usual alternative. It adds a CI pipeline and registry credentials to a one-server project, so it can wait until there is a second server. |
| How does k3s get an image Docker built? | `docker save … \| k3s ctr images import -` | k3s doesn't use Docker. It has its own containerd with its own image store. The pod says `imagePullPolicy: Never`, so a missing image fails at once instead of trying the internet. |
| What is the image called? | `abs_admin:<first 12 characters of the commit>` | A new commit gives a new name, and a new name in the manifest is what makes Kubernetes replace the pod. You can always see which commit runs, and roll back by deploying an old one. |
| Where do passwords come from? | Generated on the first deploy, stored only in k3s Secrets | Nothing secret in git or on the Mac. MySQL reads its passwords only on its first start, so they must never be regenerated: Ansible checks for the Secret first, and refuses if the data exists but the Secret doesn't. |
| How does Ansible talk to k3s? | `command: k3s kubectl …` | The same commands you'd type by hand, and no extra software. The `kubernetes.core` collection would need a Python Kubernetes client on the server. |

### Rejected alternatives

| Alternative | Why not |
|---|---|
| Ollama in k3s | 18 GB model already on disk, CPU-only, and its systemd setup is documented and works ([gemma-install.md](gemma-install.md)). |
| Traefik Ingress on port 80 | Would change the URL, and the app would still need host networking for Ollama. Add it when HTTPS comes ([open-port-8000.md](open-port-8000.md), "Adding HTTPS later"). |
| k3s's auto-deploy folder (`/var/lib/rancher/k3s/server/manifests/`) | k3s applies whatever files are in it. Simple, but Ansible can't see whether the apply worked or wait for it. |
| Helm charts | One app and one database. Plain manifests are easier to read. |
| Rsync the Mac's working copy to the server | It would deploy uncommitted work, with no record of what runs. Git makes "what's deployed" a commit. |

---
## Part 2 — Building it

Each step was run against the real server before the next one was written. One habit runs through all of
them: **run it twice**. The first run must do the work; the second must end with `changed=0`.

### Step 1 — Ansible can reach the server

Three files are the whole skeleton.

`inventory.yml` is the list of servers:

```yaml
all:
  hosts:
    hk:                            # a name for the server, used in output
      ansible_host: 47.76.236.148
      ansible_user: root
```

`ansible.cfg` points Ansible at that inventory and makes the output readable. Ansible reads it only from
the folder you run it in, which is why every command starts with `cd deployment`.

`site.yml` is the playbook. It runs one role per component, each with a tag:

```yaml
- name: Deploy abs_admin on k3s
  hosts: all
  roles:
    - { role: k3s, tags: k3s }
    - { role: ollama, tags: ollama }
    - { role: mysql, tags: mysql }
    - { role: app, tags: app }
```

Check:

```text
$ ansible all -m ping
hk | SUCCESS =>
    ping: pong
```

`ping` here is not a network ping: it logs in over SSH, runs Python, and gets an answer back. When this
works, everything else can.

The first `--tags k3s` run then failed with `The role 'ollama' was not found`. **Ansible loads every role
in a playbook, even the ones a tag skips**, so all four had to exist before any one could run.

### Step 2 — Install k3s, and skip it when installed

`roles/k3s/tasks/main.yml`, the part that does the installing:

```yaml
- name: Check which k3s is installed
  ansible.builtin.command: k3s --version
  register: k3s_installed          # keep the result: stdout, rc, ...
  changed_when: false              # only looks, never changes anything
  failed_when: false               # "command not found" is an answer, not an error
  check_mode: false                # safe to run in a --check dry run too

- name: Install k3s {{ k3s_version }}
  when: k3s_version not in k3s_installed.stdout
  block:
    - name: Download the k3s installer
      ansible.builtin.get_url:
        url: https://get.k3s.io
        dest: /usr/local/bin/k3s-install.sh
        mode: "0755"
    - name: Run the k3s installer
      ansible.builtin.command: /usr/local/bin/k3s-install.sh
      environment:
        INSTALL_K3S_VERSION: "{{ k3s_version }}"
        INSTALL_K3S_SKIP_START: "true"
      changed_when: true
      notify: Restart k3s
```

This is the "skip when installed" pattern: **look first (`register`), then act only when needed
(`when`)**. Comparing the version, rather than only checking that k3s exists, means that changing
`k3s_version` in `roles/k3s/defaults/main.yml` upgrades k3s with the same task.

The simpler form, when the version doesn't matter, is `creates:`. The Ollama role uses it:

```yaml
- name: Install Ollama
  ansible.builtin.shell: set -o pipefail && curl -fsSL https://ollama.com/install.sh | sh
  args:
    executable: /bin/bash
    creates: /usr/local/bin/ollama   # skip when this file exists
```

#### Configure before installing, restart only on change

k3s reads `/etc/rancher/k3s/config.yaml` when it starts (`roles/k3s/files/config.yaml`):

```yaml
write-kubeconfig-mode: "0600"
disable:
  - traefik       # no Ingress: the app listens on :8000 itself
  - servicelb     # no load balancer either; it would also take ports 80 and 443
flannel-backend: host-gw
```

`flannel-backend: host-gw` came from the "every port is public" finding. Flannel's default, VXLAN, opens
UDP port 8472 on every address, and that port has no login. With one server, pods never talk across
machines, so `host-gw`, which needs no port, does the same job. Check: `ss -lunp | grep 8472` prints
nothing.

The config is copied **before** the installer runs, so k3s's very first start already uses it. A changed
file must restart k3s, and that is what **handlers** are for:

```yaml
- name: Write the k3s config
  ansible.builtin.copy:
    src: config.yaml
    dest: /etc/rancher/k3s/config.yaml
  notify: Restart k3s              # queue the handler, only if this task "changed"
```

```yaml
# roles/k3s/handlers/main.yml
- name: Restart k3s
  ansible.builtin.systemd_service:
    name: k3s
    state: restarted
```

Handlers normally run at the very end of the play. Here the restart must happen before the next tasks
use k3s, so `meta: flush_handlers` runs any queued handler right there. The installer runs with
`INSTALL_K3S_SKIP_START`, and the handler starts it, so on a first install k3s starts exactly once, with
its config, however many tasks notified the handler.

Finally, wait until k3s works. Just after a start the API isn't up, and `kubectl` fails, so retry:

```yaml
- name: Wait until the node is ready
  ansible.builtin.command: k3s kubectl wait --for=condition=Ready node --all --timeout=10s
  register: node_ready
  until: node_ready.rc == 0       # retry until this is true...
  retries: 12                     # ...up to 12 times,
  delay: 10                       # 10 seconds apart
  changed_when: false
```

Check, first run (1 min 23 s, mostly the download):

```text
TASK [k3s : Write the k3s config] ......... changed
TASK [k3s : Check which k3s is installed] . ok
TASK [k3s : Download the k3s installer] ... changed
TASK [k3s : Run the k3s installer] ........ changed
RUNNING HANDLER [k3s : Restart k3s] ....... changed
TASK [k3s : Wait until the node is ready] . ok
hk : ok=11  changed=7  failed=0
```

Second run: the install block is skipped, nothing changes.

```text
TASK [k3s : Download the k3s installer] ... (skipped)
TASK [k3s : Run the k3s installer] ........ (skipped)
hk : ok=8  changed=0  skipped=2
```

And by hand on the server:

```text
$ kubectl get nodes
NAME     STATUS   ROLES           AGE   VERSION
aigent   Ready    control-plane   59s   v1.37.2+k3s1
```

Meanwhile the old app kept answering on port 8000. k3s brings its own containerd and uses its own
networks (pods `10.42.0.0/16`, services `10.43.0.0/16`), so it doesn't disturb Docker.

> **A dry run can't check a service that doesn't exist yet.** `ansible-playbook site.yml --check --diff`
> shows what would change without changing anything. On the bare server it stopped at the restart
> handler: `Could not find the requested service k3s`. In check mode the installer doesn't run, so there
> is no service. On a server that has k3s, `--check` works all the way through.

### Step 3 — The namespace and a storage class that keeps data

`roles/k3s/files/cluster.yaml` holds what the cluster needs before the workloads go in: the namespace
`abs-admin`, and a storage class.

k3s's built-in storage class `local-path` stores volumes as folders under
`/var/lib/rancher/k3s/storage/`, with `reclaimPolicy: Delete`: delete the volume claim and the folder,
the whole database, goes too. `local-path-retain` is the same class with `reclaimPolicy: Retain`, so the
folder survives. It costs four lines and protects the most valuable thing on the server.

```text
$ kubectl get storageclass
NAME                   PROVISIONER             RECLAIMPOLICY
local-path (default)   rancher.io/local-path   Delete
local-path-retain      rancher.io/local-path   Retain
```

How the manifest is applied is the subject of Step 5.

### Step 4 — Ollama

`roles/ollama/tasks/main.yml` is Steps 4 to 6 of [gemma-install.md](gemma-install.md), in Ansible:

1. Install, skipped by `creates: /usr/local/bin/ollama`.
2. Copy `override.conf` (context length 8192, keep-alive 24 h), with `notify: Restart Ollama`.
3. Start and enable the service, then wait until `http://127.0.0.1:11434/api/version` answers. The `uri`
   module calls a URL; `until` retries it.
4. If `ollama show <model>` fails, start `ollama pull` in the background with `nohup … &`, unless one is
   already running. The download takes hours at this server's bandwidth, and the deploy shouldn't wait.

Where does the model name come from? `application.json5` already says `ollama_model: "gemma4:26b"`, and
the app reads it from there. A second copy in Ansible would drift, so the role reads that file
(`roles/ollama/defaults/main.yml`):

```yaml
ollama_model: >-
  {{ lookup('file', playbook_dir ~ '/../application.json5')
     | regex_findall('ollama_model: *"([^"]+)"') | first }}
```

`lookup('file', …)` reads a file **on the Mac**. Lookups always run on the control node.

**What went wrong:** the first version used `regex_search(…, '\\1')` and failed with
`'NoneType' object has no attribute 'group'`. Backslashes go through two layers, YAML and then Jinja,
and it is hard to tell which one eats which. Tested on its own on the Mac (`hosts: localhost`), and
`regex_findall`, which returns the part in parentheses with no backslash at all, worked. When a
template expression fails, test it alone on localhost.

The first real run reported one change, the override file: it gained a `# Managed by Ansible` comment.
The handler restarted Ollama once. The second run: `changed=0`.

### Step 5 — MySQL, started once and left alone

This step is the heart of "don't restart the DB on every deploy".

#### Passwords, created exactly once

```yaml
- name: Look for MySQL's data and passwords
  ansible.builtin.command: k3s kubectl -n abs-admin get pvc/data-mysql-0 secret/mysql -o name
  register: mysql_state
  changed_when: false
  failed_when: mysql_state.rc != 0 and 'NotFound' not in mysql_state.stderr
  check_mode: false

- name: Refuse to replace the passwords of an existing database
  ansible.builtin.fail:
    msg: Secret mysql is missing, but MySQL already has data ...
  when:
    - "'persistentvolumeclaim/data-mysql-0' in mysql_state.stdout"
    - "'secret/mysql' not in mysql_state.stdout"

- name: Create MySQL's passwords (first deploy only)
  when: "'secret/mysql' not in mysql_state.stdout"
  ansible.builtin.command: k3s kubectl create -f -
  args:
    stdin: |
      apiVersion: v1
      kind: Secret
      ...
      stringData:
        password: {{ lookup('ansible.builtin.password', '/dev/null', chars=['ascii_letters', 'digits'], length=40) }}
  no_log: true
```

Points worth copying:

- `kubectl get` with two objects prints the ones that exist. One lookup answers both questions.
- `lookup('password', '/dev/null', …)` makes a random password and stores it nowhere. Letters and digits
  only, because it ends up inside a `mysql://` URL.
- The Secret goes in on **stdin**, so the password never appears on a command line, where any user on
  the server could see it in `ps`. `no_log: true` keeps it out of Ansible's output too.
- The guard matters because the MySQL image reads `MYSQL_PASSWORD` **only on its first start**. If the
  Secret were lost and recreated with new values, MySQL would still expect the old ones, and the app
  would be locked out. Failing loudly is better.

**What went wrong (found in the review after the migration):** the first version had
`failed_when: false` on the lookup and `kubectl apply` for the Secret. Picture k3s's API briefly down
during a deploy: the lookup fails, its output is empty, so "the Secret is missing", so `apply` writes
new random passwords **over the real Secret**, and the app is locked out of MySQL. Two changes close
that:

- The lookup now fails on any error except `NotFound`. "Not found" is an answer; "couldn't ask" is not.
- `kubectl create` instead of `apply`. `apply` means "make it look like this", which overwrites.
  `create` means "make a new one", and it refuses when one exists:
  `Error from server (AlreadyExists): secrets "mysql" already exists`.

The lesson: when a task can destroy something, make the destructive path need a positive answer, never
the absence of one.

#### The StatefulSet

`roles/mysql/files/mysql.yaml` has two objects:

- A **headless Service** (`clusterIP: None`). The name `mysql.abs-admin.svc.cluster.local` resolves
  straight to the pod's IP. A StatefulSet requires one, and the app connects through it.
- The **StatefulSet**: image `mysql:9.7`, passwords from the Secret, the volume from
  `volumeClaimTemplates` (`data-mysql-0`, class `local-path-retain`), and a readiness probe
  (`mysqladmin ping`). "Ready" means "accepts connections", which MySQL doesn't do while it
  initialises, so the deploy waits for real readiness. There is deliberately no liveness probe: a probe
  that fires during a slow crash recovery would kill MySQL halfway through.

The role copies the file to `/opt/abs_admin/k8s/`, applies it, and waits:

```yaml
- name: Wait until MySQL is ready
  ansible.builtin.command: k3s kubectl -n abs-admin rollout status statefulset/mysql --timeout=15m
  changed_when: false
```

First run: 4 min 30 s, mostly pulling the MySQL image at about 10 Mbps.

#### What went wrong: "configured" every time

The second run should have said `changed=0`. It said `changed=1`, and the culprit was:

```text
$ kubectl apply -f /opt/abs_admin/k8s/mysql.yaml
service/mysql unchanged
statefulset.apps/mysql configured        <- on every run
```

Had MySQL been restarted? No. The pod's age and restart count were unchanged, and so was the
StatefulSet's `metadata.generation` (it counts real spec changes). `kubectl diff`, which asks the server
what *would* change, found nothing:

```text
$ kubectl diff -f /opt/abs_admin/k8s/mysql.yaml; echo "rc=$?"
rc=0
```

So "configured" was a cosmetic quirk of client-side apply on StatefulSets: it sends a patch even when the
result is identical. Kubernetes did the right thing; only Ansible's report was wrong. But a report that
always says "changed" is useless, because you can no longer see when something really changed.

The fix is the cleanest way to say "skip if already configured": **ask `kubectl diff` first, and apply
only when it finds a difference**. `kubectl diff` exits 0 for "no difference", 1 for "different", and 2
or more for errors:

```yaml
- name: Compare the MySQL manifest with what is running
  ansible.builtin.command: k3s kubectl diff -f /opt/abs_admin/k8s/mysql.yaml
  register: mysql_diff
  changed_when: false
  failed_when: mysql_diff.rc > 1
  check_mode: false

- name: Apply the MySQL manifest
  when: mysql_diff.rc == 1
  ansible.builtin.command: k3s kubectl apply -f /opt/abs_admin/k8s/mysql.yaml
  changed_when: true
```

The same pair is used for `cluster.yaml` and the app's manifest. After the change:

```text
$ ansible-playbook site.yml --tags k3s,ollama,mysql
hk : ok=19  changed=0  skipped=8
```

and MySQL kept running, never restarted:

```text
NAME      READY   STATUS    RESTARTS   AGE
mysql-0   1/1     Running   0          4m41s
```

Does the app's network trick work? The app runs on the server's network, so the server itself was the
test:

```text
$ dig +short @10.43.0.10 mysql.abs-admin.svc.cluster.local     # 10.43.0.10 is CoreDNS
10.42.0.7
$ timeout 3 bash -c '</dev/tcp/10.42.0.7/3306' && echo ok
ok
```

### Step 6 — The app: build on the server, roll out in k3s

The role has two task files, so the migration (Step 7) can run them separately:
`roles/app/tasks/main.yml` imports `build.yml`, then `deploy.yml`.

#### build.yml: one image per commit

```yaml
- name: Get the code at {{ app_version }}
  ansible.builtin.git:
    repo: "{{ app_repo }}"
    dest: "{{ app_src_dir }}"          # /opt/abs_admin/src
    version: "{{ app_version }}"       # master, unless -e app_version=...
    force: true                        # Ansible owns this folder: local edits are discarded
  register: app_checkout

- name: Name the image after the commit
  ansible.builtin.set_fact:
    app_image: "abs_admin:{{ app_checkout.after[:12] }}"

- name: Check whether k3s already has {{ app_image }}
  ansible.builtin.command: k3s crictl inspecti {{ app_image }}
  register: image_check
  changed_when: false
  failed_when: false
  check_mode: false
```

`app_checkout.after` is the commit the checkout ended on. `set_fact` makes a variable during the run, and
`deploy.yml` puts it into the manifest. When k3s already has the image, the whole build block is
skipped. Otherwise:

```yaml
- name: Build {{ app_image }}
  ansible.builtin.command: docker build -f deployment/Dockerfile -t {{ app_image }} .
  args:
    chdir: "{{ app_src_dir }}"
  async: 3600     # allowed to run for up to an hour...
  poll: 15        # ...and Ansible checks on it every 15 seconds

- name: Copy {{ app_image }} from Docker into k3s
  ansible.builtin.shell: set -o pipefail && docker save {{ app_image }} | k3s ctr images import -
  args:
    executable: /bin/bash

- name: Remove Docker's copy of {{ app_image }}
  ansible.builtin.command: docker rmi {{ app_image }}
```

- `async`/`poll` is Ansible's tool for long tasks. Without it, one SSH command sits silent for minutes,
  and a network hiccup kills the build. With it, the build runs on its own on the server, and Ansible
  checks in.
- `set -o pipefail`: without it, a pipe's exit code is the last command's. `docker save` could fail, and
  `import` would happily report success on its empty input. `pipefail` is a bash option, hence
  `executable: /bin/bash`; Ubuntu's `/bin/sh` is dash.
- Removing Docker's copy matters, because Docker never cleans up tagged images itself. Docker's build
  cache is kept separately, so the next build still reuses the compiled dependencies.

#### deploy.yml: the manifest is a template

`roles/app/templates/app.yaml.j2` is a manifest with one variable, the image:

```yaml
      containers:
        - name: app
          image: {{ app_image }}
          imagePullPolicy: Never
```

Ansible's `template` module fills it in and writes `/opt/abs_admin/k8s/app.yaml` on the server. Run with
`--diff` and Ansible shows you the changed line before anything is applied. The rest of the manifest
holds the decisions from Part 1:

```yaml
spec:
  strategy:
    type: Recreate                       # old pod stops before the new one starts: both need :8000
  template:
    spec:
      hostNetwork: true                  # 127.0.0.1 = the server: reaches Ollama
      dnsPolicy: ClusterFirstWithHostNet # but still resolves mysql.abs-admin.svc.cluster.local
      containers:
        - env:
            - name: MYSQL_PASSWORD
              valueFrom:
                secretKeyRef: { name: mysql, key: password }
            - name: DB_URL
              value: mysql://abs_admin:$(MYSQL_PASSWORD)@mysql.abs-admin.svc.cluster.local:3306/abs_admin
```

`$(MYSQL_PASSWORD)` is a Kubernetes feature: an environment variable may use any variable defined
**above it** in the same list. The app's code is unchanged; it already took `SERVER_URL`, `DB_URL` and
`JWT_SECRET` from the environment (`src/config/config.rs`).

The rollout uses the same diff-then-apply pair as MySQL, then waits:

```yaml
- name: Roll out {{ app_image }}
  block:
    - name: Apply the app manifest
      when: app_diff.rc == 1
      ...
    - name: Wait until the new version answers
      ansible.builtin.command: k3s kubectl -n abs-admin rollout status deployment/app --timeout=5m
  rescue:
    - name: Collect the app's state
      ansible.builtin.shell: |
        k3s kubectl -n abs-admin get pods -o wide
        k3s kubectl -n abs-admin logs deployment/app --tail=30
      ...
    - name: Stop with the app's state
      ansible.builtin.fail:
        msg: "{{ app_state.stdout_lines + app_state.stderr_lines }}"
```

`rollout status` returns once the new pod passes its readiness probe (`GET /admin/` on port 8000).
**`block`/`rescue`** is Ansible's try/catch: if the rollout fails, you get the pod list and the log
right in Ansible's output, instead of a bare "timed out".

### Step 7 — Moving the live data

`migrate-from-compose.yml` is a separate playbook, run once. It reuses the roles, with the data move in
the middle, in the order that keeps downtime shortest:

```yaml
  tasks:
    - name: Set up k3s, Ollama and MySQL                 # old app still serving
      ansible.builtin.include_role:
        name: "{{ server_role }}"
      loop: [k3s, ollama, mysql]
      loop_control:
        loop_var: server_role

    - name: Build the app image                          # old app still serving
      ansible.builtin.include_role:
        name: app
        tasks_from: build                                # only roles/app/tasks/build.yml

    - name: Move the database                            # downtime starts
      when: old_mysql.stdout != ""                       # skipped once the old MySQL is gone
      block:
        - stop the old app, dump the old database
        - refuse if k3s's MySQL already has tables
        - load the dump, stop the old MySQL (down, not down -v: the volumes stay)

    - name: Start the app in k3s                         # downtime ends
      ansible.builtin.include_role:
        name: app
        tasks_from: deploy

    - copy the uploaded files into the app's new volume (rsync: copies only what's missing)
```

Design points:

- **`include_role` with `tasks_from`** runs part of a role. That's why the app role was split.
- **The old app stops first**, so nothing writes to the old database after the dump.
- **It refuses to load into a non-empty database.** Loading a dump replaces every table in it.
- **The old volumes are kept**, so the old stack can be started again ([migrate-from-compose.md](migrate-from-compose.md)).
- **Safe to run again**: once the old MySQL container is gone, the data block is skipped, and the rest
  is the normal idempotent deploy.
- **Uploads are copied after the app starts**, because the app's volume doesn't exist before that:
  `volumeBindingMode: WaitForFirstConsumer` creates it when the first pod that uses it is scheduled.

Result, 2 min 36 s in total:

```text
TASK [app : Build abs_admin:5bd761ce61be] ...... changed   (75 s: Docker reused the old build cache)
TASK [Stop the old app] ........................ changed
TASK [Dump the old database] ................... changed
TASK [Refuse to overwrite data in k3s's MySQL] . skipping
TASK [Load the dump into k3s's MySQL] .......... changed
TASK [Stop the old MySQL] ...................... changed
TASK [app : Apply the app manifest] ............ changed
TASK [app : Wait until the new version answers]  ok
    msg: abs_admin:5bd761ce61be is running at http://47.76.236.148:8000
TASK [Copy the uploaded files into the app's volume] changed
hk : ok=47  changed=12  failed=0
```

The app was down for about **15 seconds**: the dump was written at 01:38:23 UTC, and the new pod was
ready at 01:38:35.

### Step 8 — Checking the whole thing

From the Mac:

```text
$ curl http://47.76.236.148:8000/admin/
{"code":"0","msg":null,"data":"hello"}
```

On the server:

| Check | Result |
|---|---|
| `kubectl -n abs-admin get pods` | `app-…` and `mysql-0` Running, 0 restarts; MySQL 30 min old, so the migration never restarted it |
| Row counts in the new database | 1 user, 2 model versions, 1 example, 1 image, 3 permissions: the same as the old one |
| Files in the app's volume | `upload/1/…` copied, with their owner (uid 10001, the app's user) |
| The app pod reaching Ollama on `127.0.0.1:11434` | `{"version":"0.40.6"}` |
| `ss -ltnp` | 8000 public, 11434 on `127.0.0.1`, and 3306 gone from the server |
| `docker ps -a` | Empty. `docker volume ls` still lists the two old volumes. |

And the final test of the request, a full second run:

```text
$ ansible-playbook site.yml
hk : ok=29  changed=0  skipped=13          (23 s)

$ kubectl -n abs-admin get pods
NAME                   READY   STATUS    RESTARTS   AGE
app-5f457f4d88-z28mc   1/1     Running   0          76s
mysql-0                1/1     Running   0          31m
```

Everything is installed, so everything is skipped, and nothing was restarted.

---

## Part 3 — Practise

Each exercise teaches one idea from above. All are safe on this server.

1. **See a dry run.** `ansible-playbook site.yml --check --diff`. Nothing changes. Then edit a comment
   in `roles/k3s/files/config.yaml` and run it again: the diff shows the file, and the handler
   *would* restart k3s. Undo the edit.
2. **Watch a handler fire.** Make that same comment edit and run `ansible-playbook site.yml --tags k3s`
   for real. `RUNNING HANDLER [k3s : Restart k3s]` appears, and the pods keep running (`kubectl get pods
   -A`: same ages). Run it again: no handler. Undo the edit and run once more.
3. **Deploy and roll back.** Push any commit, then `ansible-playbook site.yml --tags app`. The image tag
   changes, the app pod is replaced, and `mysql-0` keeps its age. Then roll back with
   `-e app_version=<the previous commit>`. Its image is still in k3s, so there is no build, only a rollout.
4. **Read what Kubernetes stores.** `kubectl -n abs-admin get deploy app -o yaml` shows your manifest plus
   everything Kubernetes added. `kubectl -n abs-admin describe pod mysql-0` shows the Events, the first
   place to look when a pod won't start.
5. **Break it on purpose.** `kubectl -n abs-admin delete pod mysql-0`. The StatefulSet creates it again,
   with the same name, the same volume and the same data. That is self-healing, the main reason to run
   things in Kubernetes. (The app is down for the half minute MySQL takes to start.)
