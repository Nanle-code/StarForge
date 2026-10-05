/**
 * Thin wrapper around the StarForge CLI's `--json` API.
 *
 * Every public method spawns `starforge <args> --json --plain --non-interactive`,
 * parses the JSON envelope, and either resolves with the `data` payload or
 * rejects with a structured error.
 */

import * as cp from "child_process";
import * as vscode from "vscode";
import {
  JsonEnvelope,
  WalletListData,
  NetworkListData,
  DeploymentsListData,
  DeployData,
  ContractInspectData,
  ContractBuildData,
  ContractInvokeData,
  InfoData,
} from "./types";

// ---------------------------------------------------------------------------
// Configuration helpers
// ---------------------------------------------------------------------------

function binaryPath(): string {
  return (
    vscode.workspace
      .getConfiguration("starforge")
      .get<string>("binaryPath") ?? "starforge"
  );
}

function defaultNetwork(): string {
  return (
    vscode.workspace
      .getConfiguration("starforge")
      .get<string>("defaultNetwork") ?? "testnet"
  );
}

// ---------------------------------------------------------------------------
// Low-level executor
// ---------------------------------------------------------------------------

export interface CliError extends Error {
  code: string;
  fix: string;
  docs: string;
  exitCode: number;
}

function makeCliError(info: {
  code: string;
  message: string;
  fix: string;
  docs: string;
  exit_code: number;
}): CliError {
  const err = new Error(info.message) as CliError;
  err.code = info.code;
  err.fix = info.fix;
  err.docs = info.docs;
  err.exitCode = info.exit_code;
  return err;
}

/**
 * Spawn the CLI, collect stdout, parse the JSON envelope, and return `data`.
 * Rejects with a {@link CliError} for non-zero exits or `ok: false` envelopes.
 *
 * All calls add `--json --plain --non-interactive` automatically.
 */
export async function run<T>(
  args: string[],
  cwd?: string
): Promise<T> {
  const bin = binaryPath();
  const fullArgs = [...args, "--json", "--plain", "--non-interactive"];

  return new Promise<T>((resolve, reject) => {
    let stdout = "";
    let stderr = "";

    const proc = cp.spawn(bin, fullArgs, {
      cwd: cwd ?? workspaceRoot(),
      env: { ...process.env, STARFORGE_OUTPUT_JSON: "1" },
      windowsHide: true,
    });

    proc.stdout.on("data", (chunk: Buffer) => {
      stdout += chunk.toString("utf8");
    });
    proc.stderr.on("data", (chunk: Buffer) => {
      stderr += chunk.toString("utf8");
    });

    proc.on("error", (err) => {
      if ((err as NodeJS.ErrnoException).code === "ENOENT") {
        reject(
          new Error(
            `StarForge binary not found at '${bin}'. ` +
              "Install it (https://github.com/Nanle-code/StarForge#installation) " +
              "or set starforge.binaryPath in your VS Code settings."
          )
        );
      } else {
        reject(err);
      }
    });

    proc.on("close", (exitCode) => {
      // Try to parse whatever landed on stdout first — the CLI always emits
      // the JSON envelope even for non-zero exits when --json is active.
      const raw = stdout.trim();
      if (raw.startsWith("{")) {
        try {
          const envelope = JSON.parse(raw) as JsonEnvelope<T>;
          if (envelope.ok && envelope.data !== undefined) {
            resolve(envelope.data);
            return;
          }
          if (!envelope.ok && envelope.error) {
            reject(makeCliError(envelope.error));
            return;
          }
        } catch {
          // fall through to generic error
        }
      }

      // Fallback: non-JSON output or missing envelope
      const detail = stderr.trim() || raw || `exit code ${exitCode ?? "?"}`;
      reject(new Error(`starforge exited with code ${exitCode}: ${detail}`));
    });
  });
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

function workspaceRoot(): string | undefined {
  return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/** List all configured wallets. */
export async function listWallets(): Promise<WalletListData> {
  return run<WalletListData>(["wallet", "list"]);
}

/** List all configured networks. */
export async function listNetworks(): Promise<NetworkListData> {
  return run<NetworkListData>(["network", "list"]);
}

/** List deployment history (all networks). */
export async function listDeployments(
  network?: string
): Promise<DeploymentsListData> {
  const args = ["deployments", "list"];
  if (network) {
    args.push("--network", network);
  }
  return run<DeploymentsListData>(args);
}

/** Build the contract at `contractPath` (directory containing Cargo.toml). */
export async function buildContract(
  contractPath: string
): Promise<ContractBuildData> {
  return run<ContractBuildData>(
    ["contract", "build"],
    contractPath
  );
}

/** Run `cargo test` / starforge test for the contract. */
export async function testContract(
  contractPath: string
): Promise<ContractBuildData> {
  return run<ContractBuildData>(
    ["contract", "test"],
    contractPath
  );
}

/** Deploy a compiled WASM file. */
export async function deployContract(opts: {
  wasm: string;
  network?: string;
  wallet?: string;
  execute: boolean;
}): Promise<DeployData> {
  const args = [
    "deploy",
    "--wasm", opts.wasm,
    "--network", opts.network ?? defaultNetwork(),
  ];
  if (opts.wallet) {
    args.push("--wallet", opts.wallet);
  }
  if (opts.execute) {
    args.push("--execute");
  }
  return run<DeployData>(args);
}

/** Invoke a contract function. */
export async function invokeContract(opts: {
  contractId: string;
  fn: string;
  args: string[];
  network?: string;
  wallet?: string;
}): Promise<ContractInvokeData> {
  const invokeArgs = [
    "contract", "invoke",
    "--id", opts.contractId,
    "--", opts.fn,
    ...opts.args,
  ];
  // Network and source-account flags for the underlying stellar invocation
  const networkArgs: string[] = [];
  if (opts.network) {
    networkArgs.push("--network", opts.network);
  }
  if (opts.wallet) {
    networkArgs.push("--source-account", opts.wallet);
  }
  return run<ContractInvokeData>([...invokeArgs, ...networkArgs]);
}

/** Inspect a deployed contract instance. */
export async function inspectContract(
  contractId: string,
  network?: string
): Promise<ContractInspectData> {
  const args = ["contract", "inspect", "--id", contractId];
  if (network) {
    args.push("--network", network);
  }
  return run<ContractInspectData>(args);
}

/** Fetch CLI version and runtime info. */
export async function getInfo(): Promise<InfoData> {
  return run<InfoData>(["info"]);
}

/** Resolve the starforge binary version string (does not use --json). */
export async function getBinaryVersion(): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    cp.execFile(
      binaryPath(),
      ["--version"],
      { windowsHide: true },
      (err, stdout) => {
        if (err) {
          reject(err);
        } else {
          resolve(stdout.trim());
        }
      }
    );
  });
}
