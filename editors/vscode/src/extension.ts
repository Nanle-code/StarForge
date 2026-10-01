/**
 * StarForge VS Code extension — entry point.
 *
 * Activated when:
 *   - A workspace contains starforge-project.toml or Cargo.toml
 *   - Any starforge.* command is invoked
 *   - Any starforge* tree view is opened
 *
 * Contributes:
 *   - Activity-bar panel with Wallets / Networks / Deployed Contracts trees
 *   - Commands: build, test, deploy, invoke, inspect, copyContractId, showInfo
 *   - Task provider for type "starforge"
 *   - Problem matchers: $starforge-build, $starforge-warning (declared in
 *     package.json; used here by the task provider and build/test commands)
 */

import * as vscode from "vscode";
import {
  WalletsProvider,
  NetworksProvider,
  DeployedContractsProvider,
  DeployedContractItem,
} from "./treeViews";
import {
  buildContract,
  testContract,
  deployContract,
  invokeContract,
  inspectContract,
  copyContractId,
  showInfo,
  initOutputChannel,
} from "./commands";
import { StarforgeTaskProvider } from "./taskProvider";
import * as cli from "./starforgeCli";

// ---------------------------------------------------------------------------
// Activation
// ---------------------------------------------------------------------------

export async function activate(
  context: vscode.ExtensionContext
): Promise<void> {
  // Output channel — shared by commands
  const channel = initOutputChannel();
  context.subscriptions.push(channel);

  // ── Tree view providers ─────────────────────────────────────────────────
  const walletsProvider = new WalletsProvider();
  const networksProvider = new NetworksProvider();
  const contractsProvider = new DeployedContractsProvider();

  context.subscriptions.push(
    vscode.window.createTreeView("starforgeWallets", {
      treeDataProvider: walletsProvider,
      showCollapseAll: true,
    }),
    vscode.window.createTreeView("starforgeNetworks", {
      treeDataProvider: networksProvider,
      showCollapseAll: false,
    }),
    vscode.window.createTreeView("starforgeContracts", {
      treeDataProvider: contractsProvider,
      showCollapseAll: true,
    })
  );

  // ── Task provider ───────────────────────────────────────────────────────
  context.subscriptions.push(
    vscode.tasks.registerTaskProvider(
      StarforgeTaskProvider.type,
      new StarforgeTaskProvider()
    )
  );

  // ── Commands ────────────────────────────────────────────────────────────
  context.subscriptions.push(
    vscode.commands.registerCommand("starforge.build", () =>
      buildContract()
    ),

    vscode.commands.registerCommand("starforge.test", () =>
      testContract()
    ),

    vscode.commands.registerCommand("starforge.deploy", () =>
      deployContract()
    ),

    vscode.commands.registerCommand(
      "starforge.invoke",
      (item?: DeployedContractItem) => invokeContract(item)
    ),

    vscode.commands.registerCommand(
      "starforge.inspectContract",
      (item?: DeployedContractItem) => inspectContract(item)
    ),

    vscode.commands.registerCommand(
      "starforge.copyContractId",
      (item: DeployedContractItem) => copyContractId(item)
    ),

    vscode.commands.registerCommand("starforge.showInfo", () =>
      showInfo()
    ),

    // Refresh commands for each tree
    vscode.commands.registerCommand("starforge.refreshWallets", () =>
      walletsProvider.refresh()
    ),

    vscode.commands.registerCommand("starforge.refreshNetworks", () =>
      networksProvider.refresh()
    ),

    vscode.commands.registerCommand("starforge.refreshContracts", () =>
      contractsProvider.refresh()
    ),

    // Open the StarForge activity-bar panel
    vscode.commands.registerCommand(
      "starforge.openContractExplorer",
      async () => {
        await vscode.commands.executeCommand(
          "workbench.view.extension.starforge"
        );
      }
    )
  );

  // ── Auto-refresh on file-system events ─────────────────────────────────
  const autoRefresh = vscode.workspace
    .getConfiguration("starforge")
    .get<boolean>("autoRefresh", true);

  if (autoRefresh) {
    const watcher = vscode.workspace.createFileSystemWatcher(
      "**/{starforge-project.toml,*.wasm}"
    );
    context.subscriptions.push(
      watcher,
      watcher.onDidCreate(() => contractsProvider.refresh()),
      watcher.onDidChange(() => contractsProvider.refresh()),
      watcher.onDidDelete(() => contractsProvider.refresh())
    );
  }

  // ── Status-bar item ─────────────────────────────────────────────────────
  const statusBar = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    10
  );
  statusBar.command = "starforge.showInfo";
  statusBar.text = "$(circuit-board) StarForge";
  statusBar.tooltip = "StarForge — click for info";
  statusBar.show();
  context.subscriptions.push(statusBar);

  // Resolve the binary version asynchronously and update the status bar.
  void cli
    .getBinaryVersion()
    .then((version) => {
      statusBar.text = `$(circuit-board) StarForge ${version}`;
    })
    .catch(() => {
      statusBar.text = "$(circuit-board) StarForge (not found)";
      statusBar.tooltip =
        "starforge CLI not found — install it or set starforge.binaryPath";
      statusBar.backgroundColor = new vscode.ThemeColor(
        "statusBarItem.warningBackground"
      );
    });
}

// ---------------------------------------------------------------------------
// Deactivation
// ---------------------------------------------------------------------------

export function deactivate(): void {
  // Nothing to clean up — VS Code disposes all subscriptions automatically.
}
