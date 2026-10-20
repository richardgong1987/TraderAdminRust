# Opening Port 8000

On 2026-10-20 the admin app was opened to the internet at **http://47.76.236.148:8000**. This page
explains what "opening a port" means, how to check it yourself, and exactly what was changed.

## What "a port is open" means

A request from your browser has to pass three gates, from the outside in. All three must let it through.

```
 your Mac ──► ① security group ──► ② server firewall ──► ③ a program listening
              (Alibaba Cloud,       (Linux on the           on that port, on an
               outside the server)   server: ufw, nftables)  address you can reach
```

| Gate | Where it lives | How you change it |
|---|---|---|
| ① Security group | Alibaba Cloud's network, before traffic reaches the server | ECS console → the instance → Security Groups → Inbound rules |
| ② Server firewall | The Linux kernel on the server | `ufw` (simple), or `iptables` / `nft` (raw rules) |
| ③ Listener | The program itself | The program's settings: which **port** and which **address** it binds to |

Gate ③ is the one people forget. A program listens on an address *and* a port:

| Listen address | Who can connect |
|---|---|
| `127.0.0.1:8000` | Only programs on the server itself. `127.0.0.1` ("loopback", "localhost") never leaves the machine. From the internet, port 8000 looks closed. |
| `0.0.0.0:8000` | Anyone who gets past gates ① and ②. `0.0.0.0` means "every network interface on this machine". |

You can't bind to the public IP `47.76.236.148` directly, because the server doesn't have that address.
Run `ip -4 -br addr` on the server: `eth0` has `172.24.85.35`, a private address. Alibaba Cloud
translates the public IP to it before traffic arrives. `0.0.0.0` covers `eth0`, so it works.

## How to check each gate

### From outside: one test covers all three

On your Mac:

```sh
nc -vz -w 5 47.76.236.148 8000
```

| Result | Meaning |
|---|---|
| `succeeded` | All three gates are open. |
| `Connection refused`, instantly | The packet reached the server and was turned away. Gates ① and ② let it through, and nothing is listening (gate ③ is closed). |
| Waits, then `timed out` | Something dropped the packet silently, usually the security group (①) or the server firewall (②). |

A firewall can also be set to *reject* instead of drop, which also shows as "refused". `ufw status` on
the server tells you whether that's possible.

### On the server: gates ② and ③

```sh
ss -ltnp                 # ③ every listening TCP port: look at the "Local Address:Port" column
ufw status               # ② "Status: inactive" means ufw blocks nothing
iptables -S INPUT        # ② "-P INPUT ACCEPT" with no DROP or REJECT lines means nothing is blocked
```

In `ss` output, `127.0.0.1:8000` means local only, `0.0.0.0:8000` means every address, and `[::]:8000`
is the same for IPv6.

### Gate ①: the security group

The server can't see its own security group. Look in the ECS console, or use the outside test on a port
where nothing listens: **refused** means the security group let it in, **timed out** means it blocked it.

## What we found

| Gate | Check | Result |
|---|---|---|
| ① | `nc` to 80, 443, 8000 and an unused port, 12345 | All refused instantly. The security group let them through, and appears to allow **every** port. |
| ② | `ufw status`, `iptables -S INPUT` | ufw inactive, `-P INPUT ACCEPT`: nothing blocked. |
| ③ | `ss -ltnp` | The app listened on `127.0.0.1:8000`: closed to the outside. |

Only gate ③ needed changing.

## What was changed

One line in `deploy/docker-compose.yml`:

```diff
-      SERVER_URL: 127.0.0.1:8000
+      SERVER_URL: 0.0.0.0:8000
```

The app reads `SERVER_URL` at startup (`src/config/config.rs`) and listens on it
(`TcpListener::bind` in `src/main.rs`).

The container uses `network_mode: host`, so it doesn't get a network of its own. When the app listens on
`0.0.0.0:8000` inside the container, it is listening on the server's own interfaces. Nothing else in
Docker had to change.

Applied on the server:

```sh
cd /home/TraderAdminRust/deploy
sed -i 's|SERVER_URL: 127.0.0.1:8000|SERVER_URL: 0.0.0.0:8000|' docker-compose.yml
docker compose up -d     # sees the changed setting and recreates only the app container
```

No rebuild was needed. Environment variables are passed in when the container starts; they aren't part
of the image. MySQL kept running, because it belongs to a different Compose project.

## Checking that it worked

```sh
# On the server: the app now listens on every address
ss -ltnp '( sport = :8000 )'
#   LISTEN 0  128  0.0.0.0:8000  0.0.0.0:*  users:(("abs_admin",...))

# On your Mac
curl http://47.76.236.148:8000/admin/
#   {"code":"0","msg":null,"data":"hello"}
```

Then open http://47.76.236.148:8000 in a browser.

## Why Docker's `ports:` wasn't used

On a normal Docker network, a container has a private network of its own, and you publish a port with
`ports:`. MySQL works that way, in `docker-compose.mysql.yml`:

```yaml
ports:
  - "127.0.0.1:3306:3306"   # host address : host port : container port
```

`"8000:8000"` would mean every host address; `"127.0.0.1:..."` means the server's loopback only.

The app uses host networking instead, so it can reach MySQL and Ollama on `127.0.0.1`. With host
networking, Docker ignores `ports:`. The listen address inside the app is the only thing that decides.

## Things to keep in mind

- **Plain HTTP.** Your login password and token cross the internet unencrypted. That's acceptable for a
  personal tool. Change the default admin password `123456`, and don't reuse an important password.
- **Only gate ③ protects MySQL and Ollama.** The security group appears to let every port in. MySQL
  (3306) and Ollama (11434, no login at all) are safe only because they listen on `127.0.0.1`. Never
  switch them to `0.0.0.0`. To add a second layer, set the security group to allow only 22 and 8000.
- **`http://47.76.236.148` without `:8000` still doesn't work.** Browsers use port 80 for `http://`.
  The app can't listen on 80 itself, because it runs as a normal user and Linux reserves ports below 1024
  for root. That would need a reverse proxy such as Caddy or nginx on port 80.

## Closing it again

Either:

- On the server: change the line back to `127.0.0.1:8000`, then run `docker compose up -d` in
  `deploy/`. To use the app after that, tunnel over SSH:
  `ssh -N -L 8000:127.0.0.1:8000 root@47.76.236.148`, then open http://localhost:8000.
- Or, with no change on the server: add a security-group rule that denies inbound port 8000.

## Adding HTTPS later

Put a reverse proxy such as Caddy on port 443, forwarding to `127.0.0.1:8000`, and switch the app back
to `127.0.0.1:8000`. Caddy gets the certificate by itself. It needs a domain pointing at the server, or a
Let's Encrypt certificate for the bare IP (available since January 2026, valid six days, renewed
automatically).
