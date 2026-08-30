export type ContractClientOptions = {
	contractId: string;
	network?: string;
	wallet?: string;
};

export class ContractClient {
	constructor(private readonly options: ContractClientOptions) {}

	private invokeArgs(functionName: string, args: Array<[unknown, string]>): string[] {
		const cli = ["contract", "invoke", this.options.contractId, functionName, "--network", this.options.network ?? "testnet"];
		for (const [value, typeName] of args) cli.push("--arg", String(value), "--type", typeName);
		if (this.options.wallet) cli.push("--wallet", this.options.wallet, "--submit");
		return cli;
	}

	transfer(from: string, to: string, amount: number | bigint, memo: string | null): string[] /* returns CLI args; expected result: any */ {
		return this.invokeArgs("transfer", [[from, "Address"], [to, "Address"], [amount, "u128"], [memo, "Option<String>"]]);
	}

	balance_of(owner: string): string[] /* returns CLI args; expected result: number | bigint */ {
		return this.invokeArgs("balance_of", [[owner, "Address"]]);
	}

	get_metadata(): string[] /* returns CLI args; expected result: TokenMetadata */ {
		return this.invokeArgs("get_metadata", []);
	}

	batch_transfer(recipients: Array<string>, amounts: Array<number | bigint>): string[] /* returns CLI args; expected result: Array<any> */ {
		return this.invokeArgs("batch_transfer", [[recipients, "Vec<Address>"], [amounts, "Vec<u128>"]]);
	}

	set_config(key: string, value: Uint8Array): string[] /* returns CLI args; expected result: void */ {
		return this.invokeArgs("set_config", [[key, "Symbol"], [value, "Bytes"]]);
	}

}

export interface TokenMetadata {
	Name: string;
	Symbol: string;
	Decimals: number | bigint;
	TotalSupply: number | bigint;
	Admin: string;
}

export interface Allowance {
	Owner: string;
	Spender: string;
	Amount: number | bigint;
	ExpiresAt: number | bigint | null;
}

export type TokenError =
	{ type: "InsufficientBalance" } |
	{ type: "Unauthorized"; value: string } |
	{ type: "InvalidAmount"; value: number | bigint };

// Event type definitions
export interface TransferEvent {
	From: string;
	To: string;
	Amount: number | bigint;
}
