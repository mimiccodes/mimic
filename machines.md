# Machines

| Name | What | Used for |
|---|---|---|
| `github-linux-x86_64` | GitHub's standard Linux runner: 4 virtual cores (AMD EPYC or Intel Xeon, varies by run), Ubuntu | nightly runs, the speed gate on pull requests |
| `github-macos-arm64` | GitHub's standard macOS runner: virtual Apple M1, 3 cores | nightly runs, the speed gate |
| `github-windows-x86_64` | GitHub's standard Windows runner: 4 virtual cores | nightly runs, the speed gate |
| `aws-c7a.*` | AWS EC2, AMD EPYC (4th gen), one vCPU per physical core, started for one run | precise x86-64 numbers |
| `aws-c8g.*` | AWS EC2, Graviton4 (arm64), one vCPU per physical core, started for one run | precise arm64 numbers |
| `local-macos-arm64` | The maintainer's Apple M3 Pro, 12 cores, when idle | precise macOS numbers |
