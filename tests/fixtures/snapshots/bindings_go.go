package client

import "os/exec"

type ContractClientOptions struct {
	ContractID string
	Network    string
	Wallet     *string
}

type ContractClient struct {
	options ContractClientOptions
}

func NewContractClient(options ContractClientOptions) *ContractClient {
	if options.Network == "" {
		options.Network = "testnet"
	}
	return &ContractClient{options: options}
}

func (c *ContractClient) invokeArgs(functionName string, args [][2]string) []string {
	cli := []string{"contract", "invoke", c.options.ContractID, functionName, "--network", c.options.Network}
	for _, arg := range args {
		cli = append(cli, "--arg", arg[0], "--type", arg[1])
	}
	if c.options.Wallet != nil {
		cli = append(cli, "--wallet", *c.options.Wallet, "--submit")
	}
	return cli
}

func (c *ContractClient) Transfer(From string, To string, Amount string, Memo *string) []string {
	args := [][2]string{
		{"From", "Address"},
		{"To", "Address"},
		{"Amount", "u128"},
		{"Memo", "Option<String>"},
	}
	return c.invokeArgs("transfer", args)
}

func (c *ContractClient) BalanceOf(Owner string) []string {
	args := [][2]string{
		{"Owner", "Address"},
	}
	return c.invokeArgs("balance_of", args)
}

func (c *ContractClient) GetMetadata() []string {
	args := [][2]string{
	}
	return c.invokeArgs("get_metadata", args)
}

func (c *ContractClient) BatchTransfer(Recipients []string, Amounts []string) []string {
	args := [][2]string{
		{"Recipients", "Vec<Address>"},
		{"Amounts", "Vec<u128>"},
	}
	return c.invokeArgs("batch_transfer", args)
}

func (c *ContractClient) SetConfig(Key string, Value []byte) []string {
	args := [][2]string{
		{"Key", "Symbol"},
		{"Value", "Bytes"},
	}
	return c.invokeArgs("set_config", args)
}

type TokenMetadata struct {
	Name string
	Symbol string
	Decimals uint32
	TotalSupply string
	Admin string
}

type Allowance struct {
	Owner string
	Spender string
	Amount string
	ExpiresAt *uint64
}

// Event type definitions
type TransferEvent struct {
	From string
	To string
	Amount string
}
