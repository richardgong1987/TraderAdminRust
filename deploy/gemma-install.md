# Installing Gemma 4 26B A4B on the Server

This guide sets up Google's **Gemma 4 26B A4B** model on the remote server and runs it with
[Ollama](https://ollama.com). When you finish:

- Ollama runs as a system service and starts again by itself after a reboot.
- The model answers on `http://localhost:11434` on the server. That address is not reachable from the internet.
- You can use it from your Mac through an SSH tunnel.

For why this model was chosen for this server, see [The server, and why this model fits it](#the-server-and-why-this-model-fits-it).

| Step | Where | Time |
|---|---|---|
| 1. Replace the OS | Alibaba Cloud console | ~10 min |
| 2. Restore passwordless SSH | Mac | 1 min |
| 3. Update the system | Server | ~5 min |
| 4. Install Ollama | Server | ~20 min (1.4 GB download) |
| 5. Configure Ollama | Server | 1 min |
| 6. Download the model | Server | ~4.5 h at 10 Mbps (18.7 GB) |
| 7. Test it | Server | 2 min |
| 8. Use it from your Mac | Mac | 1 min |

The download times are for this server's bandwidth, about 10 Mbps. See [Network](#network-bandwidth) for how to check it, and how to download faster.

---

## The server, and why this model fits it

### Server configuration (checked 2026-10-20)

| Part | This server | What it means for running an LLM |
|---|---|---|
| Instance | Alibaba Cloud `ecs.u2i-c1m2.4xlarge`, Hong Kong | |
| CPU | Intel Xeon Platinum, 16 vCPU (8 physical cores × 2 threads), AVX-512 | The model runs on the CPU. AVX-512 speeds up the maths. Speed scales with the 8 physical cores, not the 16 vCPUs. |
| GPU | None | No GPU speed-up. Training a model is not practical on this server. |
| RAM | 30 GB, no swap | The model, its context and the OS must all fit. Going over gets the process killed; it does not just slow down. |
| Disk | 295 GB NVMe, ~281 GB free | Room for several models of 5–20 GB each. |
| Network | ~10 Mbps download (measured) | Large downloads are slow. An 18 GB model takes about 4 hours. |
| OS | Ubuntu 26.04.1 LTS, kernel 7.0, glibc 2.43, Python 3.14 | Recent enough for Ollama and current Python ML tools. |

[server-specs.md](server-specs.md) explains how to check each of these.

### What decides which model fits

Without a GPU, two things decide what works:

1. **RAM sets the largest model you can run.** The model's weights load into RAM. Each request's context
   (the "KV cache") needs more on top, and the OS and other programs need a few GB as well.
2. **Memory speed sets how fast it answers.** To produce each token, the CPU reads every *active* weight
   from RAM once. The fewer parameters a model uses per token, the faster it answers. The total parameter
   count decides how much RAM the model needs, not how fast it is.

### The Gemma 4 options on this server

| Model (Ollama tag) | Download | Parameters used per token | Fits in 30 GB? | Verdict |
|---|---|---|---|---|
| `gemma4:e2b` | 4.6–7.5 GB | ~2.3B (effective) | Easily | Fast but weakest. Most of the RAM goes unused. |
| `gemma4:e4b` | 6.6–9.5 GB | ~4.5B (effective) | Easily | Fast, but leaves ~20 GB of RAM unused that could buy quality. |
| `gemma4:12b` | 7.7–8.0 GB | Not published | Easily | Mid-size. Much smaller than the server can handle. |
| **`gemma4:26b` (26B A4B)** | **16–19 GB** | **3.8B of 25.2B (mixture of experts)** | **Yes, ~10 GB spare** | **Chosen** |
| `gemma4:31b` | 19–20 GB | All 30.7B (dense) | Barely | Reads about 8× more weights per token than 26B A4B, so it would answer several times slower. |

### Why 26B A4B makes the most of this server

- **It uses the RAM.** It is the largest Gemma 4 model that still leaves about 10 GB for context, the OS,
  and other services such as the abs_admin backend.
- **It answers at small-model speed.** A mixture-of-experts model sends each token through only a few of
  its "expert" sub-networks: 3.8B of its 25.2B parameters. Per token, the CPU reads about as much memory
  as it would for E4B, while the model holds over five times as many parameters.
- **The 31B gains little here and costs a lot.** It is dense, so every token reads all 30.7B parameters.
  On a CPU that is several times slower, and it leaves almost no RAM to spare.
- **AVX-512 and 8 cores** speed up reading the prompt. That part is limited by compute rather than memory,
  so it helps when you send long inputs such as price history or news.

What this server cannot do, whatever the model: **fine-tune it**, because that needs a GPU (rent one for
training and bring the result back here), and serve many users at the same time.

### Network bandwidth

To measure the server's download speed (it downloads 50 MB and prints the speed in bytes per second):

```sh
curl -s -o /dev/null -w '%{speed_download}\n' 'https://speed.cloudflare.com/__down?bytes=50000000'
```

Keep the quotes around the URL. Without them, the request comes back with only 1 byte.
Divide the result by 125,000 to get Mbps. On 2026-10-20 it printed `1285255`, which is about 10 Mbps. At about 10 Mbps, the model download takes hours. To make it faster, raise
the instance's public bandwidth in the ECS console before Step 6, and lower it again after the download.

---

## Why the OS must change first

The server runs **CentOS 7**, whose C library (glibc) is version **2.17**. Ollama's Linux builds are made on
AlmaLinux 8, so they need **glibc 2.28 or newer**. On CentOS 7 the binary fails with errors like
`GLIBC_2.28 not found`. CentOS 7 also stopped receiving support in June 2024.

To check a server's glibc version:

```sh
ldd --version | head -1
```

The fix is to switch the system disk to **Ubuntu 26.04 LTS**, which is what this server runs now. 24.04 LTS
works too. Ubuntu is the most common OS for this kind of tooling, and it comes with a modern Python, which
the training work will need later.

---

## Step 1 — Replace the OS (Alibaba Cloud console)

> **This erases the system disk.** On 2026-10-19 the disk held only default OS files, with no user data.
> If you have put anything on the server since then, copy it off first.
> The public IP (47.76.236.148) stays the same.

1. Log in to the [ECS console](https://ecs.console.aliyun.com) and switch the region to **China (Hong Kong)**.
2. Find the instance (`ecs.u2i-c1m2.4xlarge`) and **Stop** it. You can only replace the OS while the instance is stopped.
3. Open the instance's menu and choose **Replace Operating System** (更换操作系统).
   The menu labels change from time to time. Look under "More" or "Instance & Image".
4. Choose **Public Image**, then **Ubuntu 26.04 64-bit**. 24.04 also works.
5. Under login credentials, choose **Set Password** and set a root password.
   You need the password once, in Step 2. After that you log in with your key.
6. Confirm, wait until the instance is **Running**, and continue.

---

## Step 2 — Restore passwordless SSH (on your Mac)

The new OS has new host keys and an empty `authorized_keys`, so the old login no longer works.

```sh
# Forget the old host key. Otherwise ssh refuses with "REMOTE HOST IDENTIFICATION HAS CHANGED".
ssh-keygen -R 47.76.236.148

# Copy your public key to the server. It asks for the root password from Step 1, once.
ssh-copy-id -i ~/.ssh/id_rsa.pub root@47.76.236.148

# This should now log in without a password.
sh ssh.sh
```

Use `id_rsa.pub`, because that is the key the server accepted before.

---

## Step 3 — Update the system (on the server)

```sh
apt update && apt upgrade -y
apt install -y curl zstd
```

The Ollama download is compressed with zstd. The installer stops if `zstd` is missing.

After the upgrade, you may see a list of "outdated binaries" or "services to restart", or a purple dialog
asking which services to restart. That is normal. Press Enter to accept the defaults.

If the upgrade installed a new kernel, reboot, wait a minute, and log in again:

```sh
[ -f /var/run/reboot-required ] && reboot
```

---

## Step 4 — Install Ollama (on the server)

```sh
curl -fsSL https://ollama.com/install.sh | sh
```

The script:

- downloads Ollama (about 1.4 GB) to `/usr/local/bin/ollama` and `/usr/local/lib/ollama/`
- creates an `ollama` system user with home directory `/usr/share/ollama`, where models are stored
- creates the service file `/etc/systemd/system/ollama.service`, then enables and starts the service

The script ends with `WARNING: No NVIDIA/AMD GPU detected. Ollama will run in CPU-only mode.`
That is expected on this server.

Check that it works:

```sh
ollama --version
systemctl status ollama --no-pager     # should say "active (running)"
ss -tlnp | grep 11434                  # should listen on 127.0.0.1:11434 only
```

---

## Step 5 — Configure Ollama (on the server)

Do not edit `ollama.service` directly, because reinstalling Ollama overwrites it. Put your settings in an
override file instead:

```sh
mkdir -p /etc/systemd/system/ollama.service.d
cat > /etc/systemd/system/ollama.service.d/override.conf <<'EOF'
[Service]
Environment="OLLAMA_CONTEXT_LENGTH=8192"
Environment="OLLAMA_KEEP_ALIVE=24h"
EOF

systemctl daemon-reload
systemctl restart ollama
```

| Setting | Why |
|---|---|
| `OLLAMA_CONTEXT_LENGTH=8192` | The longest prompt plus answer, in tokens. A longer context takes more RAM. 8192 is a safe start on 30 GB. Raise it only if prompts get cut off, and watch `free -h` when you do. |
| `OLLAMA_KEEP_ALIVE=24h` | Keeps the model loaded in RAM between requests. Loading 18 GB from disk takes a while, and the default unloads it after 5 minutes. |

**Do not set `OLLAMA_HOST=0.0.0.0`.** Ollama has no login or password. If it listens on the public IP,
anyone on the internet can use the server's CPU. Use the SSH tunnel in Step 8 instead.

To see the settings the running service actually has:

```sh
systemctl show ollama --property=Environment
```

---

## Step 6 — Download the model (on the server)

```sh
nohup ollama pull gemma4:26b > /root/pull.log 2>&1 &
```

This downloads 18.7 GB, which takes about 4.5 hours at this server's bandwidth. The download is several
files, and the log shows one line per file:

| File | Size | What it is |
|---|---|---|
| `model` | 17.1 GB | The language model itself |
| `projector` | 1.2 GB | Image encoder, for questions about pictures, such as chart screenshots |
| `draft` | 0.5 GB | A small helper model |
| `template`, `license`, `params` | tiny | Prompt format and default settings | Running `ollama pull` directly would
tie the download to your SSH session: if you close the terminal or your Mac sleeps, the download stops.
`nohup ... &` runs it in the background so it keeps going after you log out.

To watch progress (Ctrl+C stops the watching, not the download):

```sh
tail -f /root/pull.log
```

If the download fails, run the same command again and it picks up where it stopped.

```sh
ollama list          # gemma4:26b should be listed
df -h /              # the model files are under /usr/share/ollama/.ollama/models
```

---

## Step 7 — Test it (on the server)

```sh
ollama run gemma4:26b --verbose "Explain what a stop-loss order is in two sentences."
```

The first run is slow because the model is loading into RAM. Run the same command again for a real speed reading.
`--verbose` prints timing stats after the answer. The line that matters:

- **`eval rate`** is how fast the answer is generated, in tokens per second. A token is about ¾ of an English word.

While the model is loaded:

```sh
ollama ps     # PROCESSOR should say "100% CPU"; UNTIL shows when it unloads
free -h       # "available" should still be several GB
```

Test the HTTP API, which is what programs use:

```sh
curl -s http://localhost:11434/api/generate -d '{
  "model": "gemma4:26b",
  "prompt": "Reply with the single word: ready",
  "stream": false
}'
```

Ollama also offers an OpenAI-compatible API at `http://localhost:11434/v1/chat/completions`. Most client
libraries can use it by changing the base URL.

---

## Step 8 — Use it from your Mac (SSH tunnel)

The tunnel forwards a port on your Mac to Ollama on the server, over the existing SSH connection.
Nothing is opened to the internet.

```sh
# Keep this running in its own terminal tab. Ctrl+C closes the tunnel.
ssh -N -L 11434:localhost:11434 root@47.76.236.148
```

Then, in another tab on your Mac:

```sh
curl -s http://localhost:11434/api/tags     # lists gemma4:26b
```

If Ollama is also installed on your Mac, port 11434 is already in use there. Use another local port, for
example `-L 11435:localhost:11434`, then call `http://localhost:11435`.

---

## Everyday commands (on the server)

| Task | Command |
|---|---|
| Service status | `systemctl status ollama --no-pager` |
| Live logs | `journalctl -u ollama -f` |
| Restart | `systemctl restart ollama` |
| Installed models | `ollama list` |
| Loaded models | `ollama ps` |
| Unload a model now (frees RAM) | `ollama stop gemma4:26b` |
| Delete a model | `ollama rm gemma4:26b` |
| Update Ollama | Run the Step 4 command again |

---

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `GLIBC_2.xx not found` | Still on CentOS 7. Do Step 1. |
| `REMOTE HOST IDENTIFICATION HAS CHANGED` | The OS was replaced. Run `ssh-keygen -R 47.76.236.148` on the Mac. |
| `ssh-copy-id` asks for a password and rejects it | The password from Step 1 is wrong, or password login is off. Reset the password in the ECS console and restart the instance. |
| Installer says `requires zstd` | `apt install -y zstd`, then run it again. |
| Answer stops halfway; log shows `signal: killed` or `out of memory` | The server ran out of RAM. There is no swap, so the kernel kills the process. Lower `OLLAMA_CONTEXT_LENGTH`, or use the smaller `gemma4:12b`. |
| First answer after a while is very slow | The model was unloaded and is loading from disk again. Check that `OLLAMA_KEEP_ALIVE` is set (Step 5). |
| `ollama` commands say they cannot connect | The service is not running. Run `systemctl restart ollama`, then read `journalctl -u ollama -n 50`. |

---

## Uninstall

```sh
systemctl disable --now ollama
rm -f /etc/systemd/system/ollama.service
rm -rf /etc/systemd/system/ollama.service.d
systemctl daemon-reload
rm -f /usr/local/bin/ollama
rm -rf /usr/local/lib/ollama
userdel ollama
rm -rf /usr/share/ollama          # also deletes all downloaded models
```
