# Reproducible WASM Builds

Hash reproducibility is central to StarForge's deploy story. To yield stable WASM hashes across different machines and environments, you must use a containerized reproducible build profile.

## Usage

We provide a Dockerfile (`docker/reproducible-wasm/Dockerfile`) and a build script to ensure that you use a consistent compiler toolchain and strict build flags.

### 1. Build the Docker Image

From the root of StarForge (or your project repository), build the builder image:

```bash
docker build -t starforge-wasm-builder docker/reproducible-wasm
```

### 2. Compile Your Contract

Mount your contract's workspace into the container and execute it. The image's entrypoint is the `build-wasm` script which enforces reproducible flags such as `--locked` and `--release`.

```bash
docker run --rm -v $(pwd):/workspace starforge-wasm-builder --release --locked
```

If you omit `--locked` or `--release`, the build will fail explicitly, as these are required to prevent non-reproducible environment variations or dependency resolutions.

## CI Verification

StarForge verifies hash equality across two completely clean builds in CI to ensure that this tooling produces stable and deterministic output.

For a mainnet release, record the reviewed artifact's SHA-256 in
`starforge-project.toml` as
`[deployment_checklist].expected_wasm_hash`. Then
`starforge deploy checklist --wasm <FILE> --network mainnet` verifies the
artifact against that release pin before the deploy gate can pass. See
[DEPLOY_CHECKLIST.md](DEPLOY_CHECKLIST.md).
