/**
 * Shared TypeScript types that mirror the StarForge CLI JSON API.
 *
 * The JSON envelope is emitted by every command that supports `--json`:
 *   starforge <command> --json
 * and has the shape:
 *   { version: 1, ok: true,  data:  <T>            }   // success
 *   { version: 1, ok: false, error: JsonErrorInfo  }   // failure
 */

// ---------------------------------------------------------------------------
// JSON envelope
// ---------------------------------------------------------------------------

export interface JsonEnvelope<T> {
  version: number;
  ok: boolean;
  data?: T;
  error?: JsonErrorInfo;
}

export interface JsonErrorInfo {
  code: string;
  message: string;
  cause: string;
  fix: string;
  docs: string;
  exit_code: number;
}

// ---------------------------------------------------------------------------
// starforge wallet list --json
// ---------------------------------------------------------------------------

export interface WalletEntry {
  name: string;
  public_key: string;
  /** secret_key is omitted from JSON output for security */
  network: string;
  created_at: string;
  funded: boolean;
}

export interface WalletListData {
  wallets: WalletEntry[];
  active_network: string;
}

// ---------------------------------------------------------------------------
// starforge network list --json  (inferred from Config.networks)
// ---------------------------------------------------------------------------

export interface NetworkEntry {
  name: string;
  horizon_url: string;
  soroban_rpc_url: string | null;
  friendbot_url: string | null;
  passphrase: string | null;
  active: boolean;
}

export interface NetworkListData {
  networks: NetworkEntry[];
  active: string;
}

// ---------------------------------------------------------------------------
// starforge deployments list --json  (DeployRecord shape)
// ---------------------------------------------------------------------------

export type DeployStatus = "success" | "failed" | "rolled-back" | "pending";

export interface DeployRecord {
  id: string;
  contract_id: string | null;
  wasm_path: string;
  wasm_hash: string;
  network: string;
  wallet: string;
  timestamp: string;
  status: DeployStatus;
  error: string | null;
  previous_id: string | null;
  approved_by: string | null;
  verification_passed: boolean;
  duration_ms: number | null;
  fee_stroops: number | null;
  note: string | null;
  changelog: string | null;
}

export interface DeploymentsListData {
  deployments: DeployRecord[];
}

// ---------------------------------------------------------------------------
// starforge deploy --json
// ---------------------------------------------------------------------------

export interface DeployData {
  wasm: string;
  network: string;
  wallet: string;
  dry_run: boolean;
  execute: boolean;
  simulated: boolean;
  success: boolean;
  contract_id: string | null;
  message: string;
}

// ---------------------------------------------------------------------------
// starforge contract inspect --json  (stable fields from cli-json-fields.json)
// ---------------------------------------------------------------------------

export interface InstanceStorageEntry {
  key: string;
  value: string;
}

export interface ContractInspectData {
  contract_id: string;
  executable: string;
  wasm_hash: string | null;
  storage_durability: string;
  latest_ledger: number;
  last_modified_ledger_seq: number | null;
  live_until_ledger_seq: number | null;
  instance_storage: InstanceStorageEntry[];
  metadata: Record<string, unknown> | string | null;
}

// ---------------------------------------------------------------------------
// starforge contract build --json  (inferred from build output)
// ---------------------------------------------------------------------------

export interface ContractBuildData {
  success: boolean;
  wasm_path: string | null;
  wasm_size_bytes: number | null;
  wasm_hash: string | null;
  duration_ms: number | null;
  errors: BuildDiagnostic[];
  warnings: BuildDiagnostic[];
}

export interface BuildDiagnostic {
  severity: "error" | "warning";
  message: string;
  code: string | null;
  file: string | null;
  line: number | null;
  column: number | null;
}

// ---------------------------------------------------------------------------
// starforge contract invoke --json
// ---------------------------------------------------------------------------

export interface ContractInvokeData {
  success: boolean;
  result: unknown;
  contract_id: string;
  function: string;
  network: string;
  fee_stroops: number | null;
  simulation_fee_stroops: number | null;
  ledger: number | null;
}

// ---------------------------------------------------------------------------
// starforge info --json
// ---------------------------------------------------------------------------

export interface InfoData {
  version: string;
  active_network: string;
  config_path: string;
  wallet_count: number;
  network_count: number;
}
