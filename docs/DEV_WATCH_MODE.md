# starforge dev watch mode

The starforge dev command provides a local contract edit loop:

1. snapshot the configured source paths;
2. run the build command;
3. deploy to the configured local network; and
4. run each configured smoke command after a successful deploy.

Create starforge-dev.toml in the project directory. Commands are argv arrays,
not shell strings, so the watcher does not invoke a shell or interpolate command
text:

    sources = ["contracts", "src"]
    build = ["stellar", "contract", "build"]
    deploy = ["stellar", "contract", "deploy", "--wasm", "target/.../contract.wasm", "--network", "standalone"]
    smoke = [["stellar", "contract", "invoke", "--id", "CONTRACT_ID", "--network", "standalone", "--", "health"]]

Run one cycle in CI or scripts with starforge dev --once. For interactive
development, run starforge dev; rapid saves are debounced with --debounce-ms
(300 ms by default), and Ctrl-C stops the watcher cleanly.

Progress is rendered as a spinner on a TTY, one stable line per event when
stdout is redirected, and one JSON object per event with --json. Step names and
errors shown in progress messages are redacted before they are emitted. Failed
builds stop the current cycle; the watcher remains alive so the next save can
retry.
