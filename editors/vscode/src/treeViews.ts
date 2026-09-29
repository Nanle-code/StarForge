/**
 * Tree-view data providers for the StarForge activity-bar panel.
 *
 * Three views are contributed:
 *   - starforgeWallets   — all wallets from `starforge wallet list --json`
 *   - starforgeNetworks  — all networks from `starforge network list --json`
 *   - starforgeContracts — deployed contracts from `starforge deployments list --json`
 */

import * as vscode from "vscode";
import * as cli from "./starforgeCli";
import { WalletEntry, NetworkEntry, DeployRecord } from "./types";

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/** Base item class that every tree leaf extends. */
class BaseItem extends vscode.TreeItem {
  constructor(
    label: string,
    collapsibleState: vscode.TreeItemCollapsibleState,
    contextValue?: string
  ) {
    super(label, collapsibleState);
    if (contextValue) {
      this.contextValue = contextValue;
    }
  }
}

// ---------------------------------------------------------------------------
// Wallets tree
// ---------------------------------------------------------------------------

export class WalletItem extends BaseItem {
  constructor(public readonly wallet: WalletEntry) {
    super(wallet.name, vscode.TreeItemCollapsibleState.Collapsed, "wallet");
    this.tooltip = `Network: ${wallet.network}\nPublic key: ${wallet.public_key}`;
    this.description = wallet.network;
    this.iconPath = new vscode.ThemeIcon(
      wallet.funded ? "key" : "key",
      wallet.funded
        ? new vscode.ThemeColor("charts.green")
        : new vscode.ThemeColor("charts.yellow")
    );
  }
}

class WalletDetailItem extends BaseItem {
  constructor(label: string, detail: string) {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.description = detail;
    this.iconPath = new vscode.ThemeIcon("symbol-field");
  }
}

export class WalletsProvider
  implements vscode.TreeDataProvider<vscode.TreeItem>
{
  private _onDidChangeTreeData = new vscode.EventEmitter<
    vscode.TreeItem | undefined | null
  >();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  private wallets: WalletEntry[] = [];
  private loading = false;
  private lastError: string | null = null;

  refresh(): void {
    this._onDidChangeTreeData.fire(undefined);
  }

  getTreeItem(element: vscode.TreeItem): vscode.TreeItem {
    return element;
  }

  async getChildren(element?: vscode.TreeItem): Promise<vscode.TreeItem[]> {
    if (element instanceof WalletItem) {
      // Detail rows under a wallet
      const w = element.wallet;
      return [
        new WalletDetailItem("Public key", w.public_key),
        new WalletDetailItem("Network", w.network),
        new WalletDetailItem("Funded", w.funded ? "yes" : "no"),
        new WalletDetailItem("Created", formatDate(w.created_at)),
      ];
    }

    if (!element) {
      // Root: fetch wallets
      if (!this.loading) {
        this.loading = true;
        this.lastError = null;
        try {
          const data = await cli.listWallets();
          this.wallets = data.wallets;
        } catch (err) {
          this.lastError = errorMessage(err);
          this.wallets = [];
        } finally {
          this.loading = false;
        }
      }

      if (this.lastError) {
        const item = new vscode.TreeItem(
          `Error: ${this.lastError}`,
          vscode.TreeItemCollapsibleState.None
        );
        item.iconPath = new vscode.ThemeIcon("error");
        item.tooltip = this.lastError;
        return [item];
      }

      if (this.wallets.length === 0) {
        const item = new vscode.TreeItem(
          "No wallets — run: starforge wallet create",
          vscode.TreeItemCollapsibleState.None
        );
        item.iconPath = new vscode.ThemeIcon("info");
        return [item];
      }

      return this.wallets.map((w) => new WalletItem(w));
    }

    return [];
  }
}

// ---------------------------------------------------------------------------
// Networks tree
// ---------------------------------------------------------------------------

export class NetworkItem extends BaseItem {
  constructor(public readonly network: NetworkEntry) {
    super(
      network.name,
      vscode.TreeItemCollapsibleState.Collapsed,
      "network"
    );
    this.description = network.active ? "active" : undefined;
    this.tooltip = network.horizon_url;
    this.iconPath = new vscode.ThemeIcon(
      network.active ? "radio-tower" : "globe",
      network.active
        ? new vscode.ThemeColor("charts.green")
        : undefined
    );
  }
}

class NetworkDetailItem extends BaseItem {
  constructor(label: string, detail: string) {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.description = detail;
    this.iconPath = new vscode.ThemeIcon("symbol-field");
  }
}

export class NetworksProvider
  implements vscode.TreeDataProvider<vscode.TreeItem>
{
  private _onDidChangeTreeData = new vscode.EventEmitter<
    vscode.TreeItem | undefined | null
  >();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  private networks: NetworkEntry[] = [];
  private loading = false;
  private lastError: string | null = null;

  refresh(): void {
    this._onDidChangeTreeData.fire(undefined);
  }

  getTreeItem(element: vscode.TreeItem): vscode.TreeItem {
    return element;
  }

  async getChildren(element?: vscode.TreeItem): Promise<vscode.TreeItem[]> {
    if (element instanceof NetworkItem) {
      const n = element.network;
      const items: vscode.TreeItem[] = [
        new NetworkDetailItem("Horizon URL", n.horizon_url),
      ];
      if (n.soroban_rpc_url) {
        items.push(new NetworkDetailItem("Soroban RPC", n.soroban_rpc_url));
      }
      if (n.friendbot_url) {
        items.push(new NetworkDetailItem("Friendbot", n.friendbot_url));
      }
      if (n.passphrase) {
        items.push(new NetworkDetailItem("Passphrase", n.passphrase));
      }
      return items;
    }

    if (!element) {
      if (!this.loading) {
        this.loading = true;
        this.lastError = null;
        try {
          const data = await cli.listNetworks();
          this.networks = data.networks;
        } catch (err) {
          this.lastError = errorMessage(err);
          this.networks = [];
        } finally {
          this.loading = false;
        }
      }

      if (this.lastError) {
        const item = new vscode.TreeItem(
          `Error: ${this.lastError}`,
          vscode.TreeItemCollapsibleState.None
        );
        item.iconPath = new vscode.ThemeIcon("error");
        return [item];
      }

      if (this.networks.length === 0) {
        const item = new vscode.TreeItem(
          "No networks configured",
          vscode.TreeItemCollapsibleState.None
        );
        item.iconPath = new vscode.ThemeIcon("info");
        return [item];
      }

      return this.networks.map((n) => new NetworkItem(n));
    }

    return [];
  }
}

// ---------------------------------------------------------------------------
// Deployed contracts tree
// ---------------------------------------------------------------------------

export class DeployedContractItem extends BaseItem {
  constructor(public readonly record: DeployRecord) {
    super(
      shortContractId(record.contract_id),
      vscode.TreeItemCollapsibleState.Collapsed,
      "deployedContract"
    );
    this.description = record.network;
    this.tooltip = buildContractTooltip(record);
    this.iconPath = statusIcon(record.status);
  }
}

class ContractDetailItem extends BaseItem {
  constructor(label: string, detail: string, icon = "symbol-field") {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.description = detail;
    this.iconPath = new vscode.ThemeIcon(icon);
  }
}

export class DeployedContractsProvider
  implements vscode.TreeDataProvider<vscode.TreeItem>
{
  private _onDidChangeTreeData = new vscode.EventEmitter<
    vscode.TreeItem | undefined | null
  >();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  private records: DeployRecord[] = [];
  private loading = false;
  private lastError: string | null = null;

  refresh(): void {
    this._onDidChangeTreeData.fire(undefined);
  }

  getTreeItem(element: vscode.TreeItem): vscode.TreeItem {
    return element;
  }

  async getChildren(element?: vscode.TreeItem): Promise<vscode.TreeItem[]> {
    if (element instanceof DeployedContractItem) {
      const r = element.record;
      const items: vscode.TreeItem[] = [
        new ContractDetailItem(
          "Contract ID",
          r.contract_id ?? "(pending)",
          "symbol-key"
        ),
        new ContractDetailItem("Network", r.network, "globe"),
        new ContractDetailItem("Status", r.status, statusIconName(r.status)),
        new ContractDetailItem("Wallet", r.wallet, "key"),
        new ContractDetailItem("Deployed", formatDate(r.timestamp), "calendar"),
        new ContractDetailItem(
          "WASM hash",
          r.wasm_hash.slice(0, 16) + "…",
          "symbol-numeric"
        ),
      ];
      if (r.duration_ms !== null) {
        items.push(
          new ContractDetailItem(
            "Duration",
            `${r.duration_ms} ms`,
            "watch"
          )
        );
      }
      if (r.fee_stroops !== null) {
        items.push(
          new ContractDetailItem(
            "Fee",
            `${r.fee_stroops} stroops`,
            "credit-card"
          )
        );
      }
      if (r.note) {
        items.push(new ContractDetailItem("Note", r.note, "note"));
      }
      return items;
    }

    if (!element) {
      if (!this.loading) {
        this.loading = true;
        this.lastError = null;
        try {
          const data = await cli.listDeployments();
          // Show most-recent first; only keep successful + pending
          this.records = data.deployments
            .filter((d) => d.status === "success" || d.status === "pending")
            .sort(
              (a, b) =>
                new Date(b.timestamp).getTime() -
                new Date(a.timestamp).getTime()
            );
        } catch (err) {
          this.lastError = errorMessage(err);
          this.records = [];
        } finally {
          this.loading = false;
        }
      }

      if (this.lastError) {
        const item = new vscode.TreeItem(
          `Error: ${this.lastError}`,
          vscode.TreeItemCollapsibleState.None
        );
        item.iconPath = new vscode.ThemeIcon("error");
        return [item];
      }

      if (this.records.length === 0) {
        const item = new vscode.TreeItem(
          "No deployments yet — use StarForge: Deploy Contract",
          vscode.TreeItemCollapsibleState.None
        );
        item.iconPath = new vscode.ThemeIcon("info");
        return [item];
      }

      return this.records.map((r) => new DeployedContractItem(r));
    }

    return [];
  }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

function shortContractId(id: string | null): string {
  if (!id) {
    return "(pending)";
  }
  return id.length > 20 ? `${id.slice(0, 8)}…${id.slice(-8)}` : id;
}

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}

function buildContractTooltip(r: DeployRecord): string {
  const lines = [
    `Contract: ${r.contract_id ?? "(pending)"}`,
    `Network:  ${r.network}`,
    `Status:   ${r.status}`,
    `Deployed: ${formatDate(r.timestamp)}`,
    `WASM:     ${r.wasm_hash}`,
  ];
  if (r.note) {
    lines.push(`Note:     ${r.note}`);
  }
  return lines.join("\n");
}

function statusIcon(
  status: DeployRecord["status"]
): vscode.ThemeIcon {
  return new vscode.ThemeIcon(
    statusIconName(status),
    statusIconColor(status)
  );
}

function statusIconName(status: DeployRecord["status"]): string {
  switch (status) {
    case "success":
      return "pass";
    case "failed":
      return "error";
    case "rolled-back":
      return "history";
    case "pending":
      return "loading~spin";
  }
}

function statusIconColor(
  status: DeployRecord["status"]
): vscode.ThemeColor | undefined {
  switch (status) {
    case "success":
      return new vscode.ThemeColor("charts.green");
    case "failed":
      return new vscode.ThemeColor("charts.red");
    case "rolled-back":
      return new vscode.ThemeColor("charts.orange");
    case "pending":
      return undefined;
  }
}

function errorMessage(err: unknown): string {
  if (err instanceof Error) {
    return err.message;
  }
  return String(err);
}
