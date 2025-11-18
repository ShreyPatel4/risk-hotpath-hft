# risk-hotpath-hft

A lightweight Rust workspace for high-frequency trading risk experiments. The project ships with
a simulator-driven binary, ClickHouse/Prometheus/Grafana observability stack, and simple Make
targets to get started quickly.

## Getting started

Install Rust (stable) and Docker. The supplied targets assume a local toolchain and Docker daemon
running.

```bash
make up           # start ClickHouse, Prometheus, and Grafana
make run_sim      # run the simulator binary
make bench        # compile and run benchmark harness
make replay       # run the replay mode against sample data
make export       # export metrics from the running binary
```

Prometheus exposes metrics on `9898` by default. Grafana ships with a placeholder dashboard you can
import from `grafana/provisioning/dashboards/dashboard.json`.
