# Firecracker — production isolation

The process provider is for laptops. Production agent execution must run
inside real isolation boundaries. Bulkhead's design keeps the gateway code
identical either way: a `FirecrackerProvider` implements the same
`SandboxProvider` trait (see `crates/bulkhead-providers/src/lib.rs`) and the
operator points the gateway at it.

## Why Firecracker

- MicroVMs with hardware-virtualized isolation, the same machinery behind
  Vercel Sandbox and Fly.io Sprites.
- ~125 ms cold starts and snapshot/restore — the "stateful agent era"
  primitive.
- Apache-2.0, no kernel modules required (KVM), runs on any Linux fleet you
  already own. No per-sandbox bill: the marginal cost of the thousandth
  microVM on your own box is zero.

## Deployment shape (reference)

```text
                        ┌───────────────────────────────┐
 agents ──▶ bulkhead-gateway │ policy / identity / budget / │
 (HTTP)    (your fleet)  │ audit                        │
                        └──────────────┬────────────────┘
                                       │ allow only
                        ┌──────────────▼───────────────┐
                        │ firecracker jailer           │
                        │  /srv/jailer/firecracker     │
                        │  per-agent microVM:          │
                        │   · 1 vCPU / 256 MiB default │
                        │   · deny-all egress by default│
                        │   · writable /work only      │
                        │   · kernel = vmlinux.bin     │
                        └──────────────────────────────┘
```

Reference `FirecrackerProvider` config (YAML sketch; the provider reads the
same shape):

```yaml
provider: firecracker
jailer_binary: /usr/local/bin/jailer
firecracker_binary: /usr/local/bin/firecracker
kernel: /var/lib/bulkhead/vmlinux.bin
rootfs: /var/lib/bulkhead/rootfs.ext4
socket_dir: /run/bulkhead/vm
defaults:
  cpu_millis: 1000
  mem_mb: 256
  egress: deny            # per-VM tap device with a default-deny filter
```

## Hardening checklist (before production)

- [ ] Run the jailer as an unprivileged user with KVM access via group.
- [ ] Deny-all egress at the tap device; only the gateway's audit collector
      is reachable.
- [ ] Mount `/work` as writable tmpfs; everything else read-only.
- [ ] Cap VM lifetime (`--budget` at the gateway, `max_run` in the provider).
- [ ] Red-team pass: attempt escape, credential theft, and audit tampering.
      Publish the report — in this market, proof of safety is marketing.

## What does NOT change

The gateway, the policy format, the SDKs, the audit trail, and the budget
kill switch are provider-agnostic by construction. Firecracker is an
implementation detail of one `SandboxProvider`.
