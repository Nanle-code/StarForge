/**
 * VS Code command handlers for StarForge.
 *
 * build    — runs `starforge contract build` as a VS Code Task (shows
 *            output in the Terminal panel with problem-matcher diagnostics).
 * test     — runs `starforge contract test` as a VS Code Task.
 * deploy   — interactive wizard: pick WASM file → pick network → pick wallet
 *            → confirm → run `starforge deploy --execute`.
 * invoke   — interactive wizard: pick contract → enter function name → enter
 *            args → run `starforge contract invoke`.
 */

import * as path from "path";
import * as vscode from "vscode";
import * as cli from "./starforgeCli";
import {
  DeployedContractItem,
} from "./treeViews";

// ---------------------------------------------------------------------------
// Re-exported output channel (shared with extension.ts)
// ---------------------------------------------------------------------------

export let outputChannel: vscode.OutputChannel;

export function initOutputChannel(): vscode.OutputChannel {
  outputChannel = vscode.window.createOutputChannel("StarForge");
  return outputChannel;
}

// ---------------------------------------------------------------------------
// build
// ---------------------------------------------------------------------------

/**
 * Run `starforge contract build` as a VS Code Task so that the terminal panel
 * captures output and the `starforge-build` problem matcher populates the
 * Problems view.
 */
export async function buildContract(): Promise<void> {
  const folder = await pickWorkspaceFolder("Select the workspace to build");
  if (!folder) {
    return;
  }

  const bin = getBinaryPath();
  const taskDef: vscode.TaskDefinition = { type: "starforge", task: "build" };
  const shellExec = new vscode.ShellExecution(
    bin,
    ["contract", "build"],
    { cwd: folder.uri.fsPath }
  );
  const task = new vscode.Task(
    taskDef,
    folder,
    "build",
    "StarForge",
    shellExec,
    ["$starforge-build", "$starforge-warning"]
  );
  task.group = vscode.TaskGroup.Build;
  task.presentationOptions = {
    reveal: vscode.TaskRevealKind.Always,
    panel: vscode.TaskPanelKind.Shared,
    showReuseMessage: false,
  };

  await vscode.tasks.executeTask(task);
}

// ---------------------------------------------------------------------------
// test
// ---------------------------------------------------------------------------

export async function testContract(): Promise<void> {
  const folder = await pickWorkspaceFolder("Select the workspace to test");
  if (!folder) {
    return;
  }

  const bin = getBinaryPath();
  const taskDef: vscode.TaskDefinition = { type: "starforge", task: "test" };
  const shellExec = new vscode.ShellExecution(
    bin,
    ["contract", "test"],
    { cwd: folder.uri.fsPath }
  );
  const task = new vscode.Task(
    taskDef,
    folder,
    "test",
    "StarForge",
    shellExec,
    ["$starforge-build", "$starforge-warning"]
  );
  task.group = vscode.TaskGroup.Test;
  task.presentationOptions = {
    reveal: vscode.TaskRevealKind.Always,
    panel: vscode.TaskPanelKind.Shared,
    showReuseMessage: false,
  };

  await vscode.tasks.executeTask(task);
}

// ---------------------------------------------------------------------------
// deploy
// ---------------------------------------------------------------------------

export async function deployContract(): Promise<void> {
  // Step 1 — pick WASM file
  const wasmUris = await vscode.window.showOpenDialog({
    title: "Select compiled .wasm file",
    filters: { "WASM files": ["wasm"], "All files": ["*"] },
    canSelectMany: false,
    openLabel: "Deploy",
  });
  if (!wasmUris || wasmUris.length === 0) {
    return;
  }
  const wasmPath = wasmUris[0].fsPath;

  // Step 2 — pick network
  const network = await pickNetwork();
  if (!network) {
    return;
  }

  // Step 3 — pick wallet (optional)
  const wallet = await pickWallet();
  // wallet === undefined → user cancelled; wallet === "" → use default
  if (wallet === undefined) {
    return;
  }

  // Step 4 — dry-run preview
  const dryRunChoice = await vscode.window.showInformationMessage(
    `Deploy ${path.basename(wasmPath)} to ${network}?`,
    { modal: true },
    "Preview (dry-run)",
    "Deploy now"
  );
  if (!dryRunChoice) {
    return;
  }

  const execute = dryRunChoice === "Deploy now";

  await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: execute ? "Deploying contract…" : "Running dry-run…",
      cancellable: false,
    },
    async () => {
      try {
        const result = await cli.deployContract({
          wasm: wasmPath,
          network,
          wallet: wallet || undefined,
          execute,
        });

        if (execute && result.success && result.contract_id) {
          void vscode.window.showInformationMessage(
            `Contract deployed! ID: ${result.contract_id}`,
            "Copy ID"
          ).then((choice) => {
            if (choice === "Copy ID") {
              void vscode.env.clipboard.writeText(result.contract_id!);
            }
          });
        } else if (!execute) {
          outputChannel.appendLine(
            `[dry-run] ${result.message ?? "No issues found."}`
          );
          outputChannel.show(true);
          void vscode.window.showInformationMessage(
            "Dry-run complete — see StarForge output for details."
          );
        }
      } catch (err) {
        void vscode.window.showErrorMessage(
          `Deploy failed: ${errorMessage(err)}`
        );
        outputChannel.appendLine(`[deploy error] ${errorMessage(err)}`);
        outputChannel.show(true);
      }
    }
  );
}

// ---------------------------------------------------------------------------
// invoke
// ---------------------------------------------------------------------------

export async function invokeContract(
  contextItem?: DeployedContractItem
): Promise<void> {
  // Step 1 — contract ID
  let contractId = contextItem?.record.contract_id ?? null;
  if (!contractId) {
    contractId = await pickContractId();
    if (!contractId) {
      return;
    }
  }

  // Step 2 — function name
  const fn = await vscode.window.showInputBox({
    title: "Function name",
    prompt: "Name of the Soroban function to invoke",
    placeHolder: "e.g. increment",
    validateInput: (v) => (v.trim() ? undefined : "Function name is required"),
  });
  if (!fn) {
    return;
  }

  // Step 3 — arguments (space-separated, optional)
  const argsRaw = await vscode.window.showInputBox({
    title: "Arguments",
    prompt:
      "Space-separated arguments in Soroban XDR/scalar form (leave blank for none)",
    placeHolder: "e.g. --arg-name val1 --arg-name2 val2",
  });
  if (argsRaw === undefined) {
    // cancelled
    return;
  }
  const args = argsRaw.trim() ? argsRaw.trim().split(/\s+/) : [];

  // Step 4 — network
  const network = await pickNetwork();
  if (!network) {
    return;
  }

  // Step 5 — wallet (source account)
  const wallet = await pickWallet();
  if (wallet === undefined) {
    return;
  }

  await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: `Invoking ${fn}…`,
      cancellable: false,
    },
    async () => {
      try {
        const result = await cli.invokeContract({
          contractId: contractId!,
          fn,
          args,
          network,
          wallet: wallet || undefined,
        });

        const resultStr =
          typeof result.result === "string"
            ? result.result
            : JSON.stringify(result.result, null, 2);

        outputChannel.appendLine(`\n[invoke] ${fn}(${args.join(", ")})`);
        outputChannel.appendLine(`Contract: ${contractId}`);
        outputChannel.appendLine(`Network:  ${result.network}`);
        outputChannel.appendLine(`Result:   ${resultStr}`);
        if (result.fee_stroops !== null) {
          outputChannel.appendLine(`Fee:      ${result.fee_stroops} stroops`);
        }
        outputChannel.show(true);

        void vscode.window.showInformationMessage(
          `${fn}() → ${resultStr.slice(0, 60)}${resultStr.length > 60 ? "…" : ""}`
        );
      } catch (err) {
        void vscode.window.showErrorMessage(
          `Invoke failed: ${errorMessage(err)}`
        );
        outputChannel.appendLine(`[invoke error] ${errorMessage(err)}`);
        outputChannel.show(true);
      }
    }
  );
}

// ---------------------------------------------------------------------------
// inspect
// ---------------------------------------------------------------------------

export async function inspectContract(
  contextItem?: DeployedContractItem
): Promise<void> {
  let contractId = contextItem?.record.contract_id ?? null;
  if (!contractId) {
    contractId = await pickContractId();
    if (!contractId) {
      return;
    }
  }

  const network = await pickNetwork();
  if (!network) {
    return;
  }

  await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: "Inspecting contract…",
      cancellable: false,
    },
    async () => {
      try {
        const data = await cli.inspectContract(contractId!, network);
        const json = JSON.stringify(data, null, 2);

        // Open result in a virtual document
        const doc = await vscode.workspace.openTextDocument({
          language: "json",
          content: json,
        });
        await vscode.window.showTextDocument(doc, {
          preview: true,
          viewColumn: vscode.ViewColumn.Beside,
        });
      } catch (err) {
        void vscode.window.showErrorMessage(
          `Inspect failed: ${errorMessage(err)}`
        );
      }
    }
  );
}

// ---------------------------------------------------------------------------
// copyContractId
// ---------------------------------------------------------------------------

export async function copyContractId(
  item: DeployedContractItem
): Promise<void> {
  const id = item.record.contract_id;
  if (!id) {
    void vscode.window.showWarningMessage("Contract ID not available yet.");
    return;
  }
  await vscode.env.clipboard.writeText(id);
  void vscode.window.showInformationMessage(`Copied: ${id}`);
}

// ---------------------------------------------------------------------------
// showInfo
// ---------------------------------------------------------------------------

export async function showInfo(): Promise<void> {
  try {
    const info = await cli.getInfo();
    outputChannel.appendLine("\n[info]");
    outputChannel.appendLine(`Version:        ${info.version}`);
    outputChannel.appendLine(`Active network: ${info.active_network}`);
    outputChannel.appendLine(`Config:         ${info.config_path}`);
    outputChannel.appendLine(`Wallets:        ${info.wallet_count}`);
    outputChannel.appendLine(`Networks:       ${info.network_count}`);
    outputChannel.show(true);
  } catch (err) {
    void vscode.window.showErrorMessage(
      `starforge info failed: ${errorMessage(err)}`
    );
  }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

function getBinaryPath(): string {
  return (
    vscode.workspace
      .getConfiguration("starforge")
      .get<string>("binaryPath") ?? "starforge"
  );
}

async function pickWorkspaceFolder(
  placeholder: string
): Promise<vscode.WorkspaceFolder | undefined> {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    void vscode.window.showWarningMessage(
      "No workspace folder open. Open a StarForge project first."
    );
    return undefined;
  }
  if (folders.length === 1) {
    return folders[0];
  }
  return vscode.window.showWorkspaceFolderPick({ placeHolder: placeholder });
}

async function pickNetwork(): Promise<string | undefined> {
  try {
    const data = await cli.listNetworks();
    const items = data.networks.map((n) => ({
      label: n.name,
      description: n.active ? "active" : undefined,
      detail: n.horizon_url,
    }));
    const pick = await vscode.window.showQuickPick(items, {
      title: "Select network",
      placeHolder: "Choose the target network",
    });
    return pick?.label;
  } catch {
    // Fallback: manual entry
    return vscode.window.showInputBox({
      title: "Network",
      value: "testnet",
      placeHolder: "testnet or mainnet",
    });
  }
}

/**
 * Returns the selected wallet name, an empty string if the user wants the
 * default wallet, or undefined if the picker was cancelled.
 */
async function pickWallet(): Promise<string | undefined> {
  try {
    const data = await cli.listWallets();
    const items: vscode.QuickPickItem[] = [
      { label: "(default)", description: "Use the CLI default wallet" },
      ...data.wallets.map((w) => ({
        label: w.name,
        description: w.network,
        detail: w.public_key,
      })),
    ];
    const pick = await vscode.window.showQuickPick(items, {
      title: "Select wallet",
      placeHolder: "Choose the signing wallet",
    });
    if (!pick) {
      return undefined;
    }
    return pick.label === "(default)" ? "" : pick.label;
  } catch {
    return vscode.window.showInputBox({
      title: "Wallet name",
      placeHolder: "Leave blank for default",
    });
  }
}

async function pickContractId(): Promise<string | null> {
  // Try to offer known contract IDs from deployment history
  try {
    const data = await cli.listDeployments();
    const deployed = data.deployments.filter(
      (d) => d.status === "success" && d.contract_id
    );
    if (deployed.length > 0) {
      const items = deployed.map((d) => ({
        label: d.contract_id!,
        description: d.network,
        detail: `Deployed ${new Date(d.timestamp).toLocaleString()}`,
      }));
      const pick = await vscode.window.showQuickPick(items, {
        title: "Select contract",
        placeHolder: "Choose a deployed contract or type an ID",
      });
      if (pick) {
        return pick.label;
      }
    }
  } catch {
    // fall through to manual entry
  }

  return (
    (await vscode.window.showInputBox({
      title: "Contract ID",
      prompt: "Enter the Stellar contract ID (C...)",
      validateInput: (v) =>
        v.trim().startsWith("C") && v.trim().length > 10
          ? undefined
          : "Enter a valid Soroban contract ID (starts with C)",
    })) ?? null
  );
}

function errorMessage(err: unknown): string {
  if (err instanceof Error) {
    return err.message;
  }
  return String(err);
}
