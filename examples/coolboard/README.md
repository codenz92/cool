# coolboard

Small packaged service example for Phase 17.

- `cool src/main.cool` starts the service on `127.0.0.1:8081`
- `COOLBOARD_PORT=9090 cool src/main.cool` changes the port
- `COOLBOARD_DB=/tmp/coolboard.sqlite cool src/main.cool` changes the SQLite path
- `cool ../../apps/pulse.cool --file pulse.toml` runs concurrent health checks against it
- `cool ../../apps/control.cool --file pulse.toml` opens the dashboard view
