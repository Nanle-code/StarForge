# StarForge Error Catalog

Stable error codes, causes, fixes, and process exit statuses emitted by the CLI.

### SF0001: GeneralFailure

- Domain: general
- Exit: 1 (GENERAL_FAILURE)
- Cause: The command failed for an unclassified reason.
- Fix: Retry with --json and include the full error and correlation ID in a support report.

### SF0002: GeneralUsageFailure

- Domain: general
- Exit: 2 (USAGE_ERROR)
- Cause: The command arguments or input values are invalid.
- Fix: Check the command's `--help` output and correct the arguments or input values.

### SF0003: GeneralConfigFailure

- Domain: general
- Exit: 3 (CONFIG_ERROR)
- Cause: The local StarForge configuration is missing or invalid.
- Fix: Run `starforge info` and correct the configuration file or selected settings.

### SF0004: GeneralSigningFailure

- Domain: general
- Exit: 5 (SIGNING_ERROR)
- Cause: A signing key, secret, or cryptographic operation failed.
- Fix: Verify the configured key and passphrase, then retry the signing operation.

### SF0005: GeneralEnvironmentFailure

- Domain: general
- Exit: 7 (ENVIRONMENT_ERROR)
- Cause: A required system dependency or permission is unavailable.
- Fix: Install the required dependency or correct filesystem permissions, then retry.

### SF0006: GeneralBreakingChange

- Domain: general
- Exit: 8 (BREAKING_INTERFACE_CHANGE)
- Cause: The contract interface contains an unacknowledged breaking change.
- Fix: Review the interface diff and pass `--acknowledge` only after approving the change.

### SF0007: GeneralExecutionFailure

- Domain: general
- Exit: 6 (EXECUTION_ERROR)
- Cause: Contract execution, compilation, or verification failed.
- Fix: Review the build or execution diagnostics, then correct the contract and retry.

### SF1001: WalletNotFound

- Domain: wallet
- Exit: 3 (CONFIG_ERROR)
- Cause: The requested wallet is not present in the local configuration.
- Fix: Run `starforge wallet list` and use an existing wallet name, or create it with `starforge wallet create <name>`.

### SF1002: WalletSecretMissing

- Domain: wallet
- Exit: 5 (SIGNING_ERROR)
- Cause: The wallet has no locally stored signing secret.
- Fix: Import or create a wallet with a signing secret, then retry the command.

### SF1003: WalletSigningFailed

- Domain: wallet
- Exit: 5 (SIGNING_ERROR)
- Cause: The wallet secret could not be unlocked or used to sign.
- Fix: Verify the wallet passphrase and signing key, then retry with the intended network.

### SF1099: WalletOperationFailed

- Domain: wallet
- Exit: 3 (CONFIG_ERROR)
- Cause: The wallet command could not complete its requested operation.
- Fix: Run `starforge wallet --help` and verify the wallet name and required options.

### SF1101: NetworkUnavailable

- Domain: network
- Exit: 4 (NETWORK_ERROR)
- Cause: The configured Stellar network endpoint could not be reached.
- Fix: Check the endpoint URL and connectivity, then retry with `starforge network test`.

### SF1102: NetworkRequestFailed

- Domain: network
- Exit: 4 (NETWORK_ERROR)
- Cause: The network endpoint returned an unsuccessful or invalid response.
- Fix: Run `starforge network test` and retry after the endpoint returns a valid response.

### SF1199: NetworkOperationFailed

- Domain: network
- Exit: 4 (NETWORK_ERROR)
- Cause: The network command could not complete its requested operation.
- Fix: Run `starforge network show` to verify the active network and endpoint settings.

### SF1201: DeployArtifactInvalid

- Domain: deploy
- Exit: 6 (EXECUTION_ERROR)
- Cause: The deployment WASM artifact is missing or invalid.
- Fix: Build the contract and pass an existing valid `.wasm` file with `--wasm`.

### SF1202: DeploySimulationFailed

- Domain: deploy
- Exit: 6 (EXECUTION_ERROR)
- Cause: Soroban RPC could not simulate the deployment transaction.
- Fix: Check Soroban RPC connectivity and simulation diagnostics, then retry the deployment.

### SF1203: DeploySubmissionFailed

- Domain: deploy
- Exit: 6 (EXECUTION_ERROR)
- Cause: The deployment transaction was rejected or failed during submission.
- Fix: Review the transaction response, wallet balance, and network before retrying.

### SF1299: DeployOperationFailed

- Domain: deploy
- Exit: 6 (EXECUTION_ERROR)
- Cause: The deployment command could not complete its requested operation.
- Fix: Run `starforge deploy --help` and verify the artifact, wallet, and network options.

### SF1301: TemplateNotFound

- Domain: template
- Exit: 3 (CONFIG_ERROR)
- Cause: The requested template is not available in the selected source.
- Fix: Check the template name and source with `starforge template list` or `starforge registry search`.

### SF1302: TemplateInvalid

- Domain: template
- Exit: 2 (USAGE_ERROR)
- Cause: The template does not satisfy the required schema or validation rules.
- Fix: Validate the template manifest against the documented schema and correct reported fields.

### SF1399: TemplateOperationFailed

- Domain: template
- Exit: 3 (CONFIG_ERROR)
- Cause: The template command could not complete its requested operation.
- Fix: Run `starforge template --help` and verify the template name and requested operation.

### SF1401: PluginUnavailable

- Domain: plugin
- Exit: 7 (ENVIRONMENT_ERROR)
- Cause: The requested plugin could not be found or loaded.
- Fix: Install or enable the plugin, then verify it with `starforge plugin list`.

### SF1402: PluginExecutionFailed

- Domain: plugin
- Exit: 7 (ENVIRONMENT_ERROR)
- Cause: The plugin returned an error while executing the command.
- Fix: Check the plugin's diagnostic output and compatibility, then retry the command.

### SF1499: PluginOperationFailed

- Domain: plugin
- Exit: 7 (ENVIRONMENT_ERROR)
- Cause: The plugin command could not complete its requested operation.
- Fix: Run `starforge plugin --help` and verify the plugin name and command arguments.