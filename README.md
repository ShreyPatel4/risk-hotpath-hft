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

## Project bootstrap helper

The `scripts/bootstrap_project.py` helper wires up a GitHub Projects v2 board and seeds the four
starter issues described in the brief. It will:

- Create a project (using the git remote owner by default)
- Add custom fields Track, Priority, Size, Stage, and Sprint (Sprint defaults to `Sprint 1`)
- Create the four backlog issues via `gh issue create`
- Add each issue to the project and populate the custom fields

Usage:

```bash
export GITHUB_TOKEN=<token-with-project-and-issue-scope>
./scripts/bootstrap_project.py \
  --owner <github-owner> \
  --repo risk-hotpath-hft \
  --project-title "Risk hotpath HFT" \
  --sprint "Sprint 1"
```

By default the owner and repo are inferred from `git remote origin`. The script prints the project
URL and the URLs of the created issues when complete.
