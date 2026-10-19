# Checking the Server's Hardware

How to find out what the remote server has: CPU, memory, disk, GPU and OS.
Every command here only reads information. None of them change the server.

## Connect

`ssh.sh` in the project root logs in as root. The server has key-based login, so no password is needed.

```sh
sh ssh.sh
```

To run one command without opening a shell:

```sh
ssh root@47.76.236.148 'free -h'
```

## Quick summary

Paste this into the server to print everything at once:

```sh
echo "== OS";     cat /etc/os-release | head -2; uname -r
echo "== CPU";    lscpu | grep -E '^Model name|^CPU\(s\)|^Thread|^Core|^Socket'
echo "== Memory"; free -h
echo "== Disk";   df -h -x tmpfs -x devtmpfs
echo "== GPU";    nvidia-smi 2>/dev/null || lspci | grep -iE 'vga|3d|nvidia'
```

## CPU

```sh
nproc        # number of logical CPUs (vCPUs)
lscpu        # full details
```

The lines in `lscpu` that matter:

| Line | Meaning |
|---|---|
| `CPU(s)` | Logical CPUs, the number the cloud console calls "vCPU" |
| `Thread(s) per core` | `2` means hyper-threading is on |
| `Core(s) per socket` × `Socket(s)` | Physical cores |
| `Model name` | The processor model |

Physical cores = `CPU(s)` ÷ `Thread(s) per core`. For example, 16 vCPU with 2 threads per core means 8 physical cores.
Software that is limited by compute, such as running an LLM on the CPU, scales with physical cores, not vCPUs.

To check for AVX-512, which makes running models on the CPU faster:

```sh
grep -o -m1 avx512f /proc/cpuinfo || echo "no AVX-512"
```

## Memory

```sh
free -h
```

```
              total        used        free      shared  buff/cache   available
Mem:            30G        578M         29G        444K        319M         29G
Swap:            0B          0B          0B
```

- `total` is the installed RAM.
- `available` is how much a new program can actually use. Read this column, not `free`: Linux uses spare RAM as disk cache (`buff/cache`) and gives it back when a program needs it.
- `Swap: 0B` means there is no swap. If a program uses up the RAM, the kernel kills it instead of slowing down.

## Disk

```sh
df -h -x tmpfs -x devtmpfs   # mounted filesystems: size, used, free
lsblk                        # physical disks, including unmounted ones
```

```
Filesystem      Size  Used Avail Use% Mounted on
/dev/nvme0n1p1  296G  2.4G  281G   1% /
```

`df` shows only mounted filesystems. A data disk attached in the cloud console shows up in `lsblk`, but not in `df` until it is formatted and mounted.

`lsblk` reports the raw disk (300G here), and `df` reports the usable filesystem (296G). The difference is filesystem overhead, not a missing disk.

To see what is using the space:

```sh
du -sh /* 2>/dev/null | sort -h | tail
```

## GPU

```sh
nvidia-smi
```

If `nvidia-smi` is missing, either there is no NVIDIA GPU or the driver is not installed. To tell which:

```sh
lspci | grep -iE 'vga|3d|nvidia'
```

An NVIDIA line means a GPU is there but has no driver. If the only line is a virtual display adapter such as `Cirrus Logic GD 5446`, the server has no GPU.

## OS

```sh
cat /etc/os-release   # distribution and version
uname -r              # kernel version
python3 --version
```

## Cloud instance type (Alibaba Cloud)

The instance type maps directly to the specs on Alibaba's pricing page. Only run these from inside the server:

```sh
curl -s http://100.100.100.200/latest/meta-data/instance/instance-type; echo
curl -s http://100.100.100.200/latest/meta-data/region-id; echo
```

## Current server (checked 2026-10-19)

| | |
|---|---|
| Instance | `ecs.u2i-c1m2.4xlarge`, `cn-hongkong` |
| CPU | Intel Xeon Platinum, 16 vCPU (8 cores × 2 threads), AVX-512 |
| Memory | 30 GB, no swap |
| Disk | 296 GB NVMe, 2.4 GB used |
| GPU | None |
| OS | CentOS 7, kernel 3.10, Python 3.6.8 |

Re-run the quick summary after any upgrade or OS change, and update this table.

┌──────────┬───────────────────────────────────────────────────────────────────────────────────┐
│          │                                                                                   │
├──────────┼───────────────────────────────────────────────────────────────────────────────────┤
│ Instance │ Alibaba Cloud ecs.u2i-c1m2.4xlarge, Hong Kong                                     │
├──────────┼───────────────────────────────────────────────────────────────────────────────────┤
│ CPU      │ 16 vCPU (8 cores × 2 threads), Xeon Platinum, has AVX‑512                         │
├──────────┼───────────────────────────────────────────────────────────────────────────────────┤
│ RAM      │ 30 GB, no swap                                                                    │
├──────────┼───────────────────────────────────────────────────────────────────────────────────┤
│ Disk     │ 296 GB NVMe, 2.4 GB used                                                          │
├──────────┼───────────────────────────────────────────────────────────────────────────────────┤
│ GPU      │ None. The only display device is a virtual Cirrus Logic VGA adapter.              │
├──────────┼───────────────────────────────────────────────────────────────────────────────────┤
│ OS       │ CentOS 7 (kernel 3.10), Python 3.6.8. Docker, conda and Ollama are not installed. │
