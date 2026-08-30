from dataclasses import dataclass
from typing import List, Dict, Optional, Union, Tuple
import subprocess

@dataclass
class ContractClientOptions:
contract_id: str
network: str = "testnet"
wallet: Optional[str] = None

class ContractClient:
def __init__(self, options: ContractClientOptions):
self.options = options

def _invoke_args(self, function_name: str, args: List[Tuple[str, str]]) -> List[str]:
cli = ["starforge", "contract", "invoke", self.options.contract_id, function_name, "--network", self.options.network]
for value, type_name in args:
cli.extend(["--arg", str(value), "--type", type_name])
if self.options.wallet:
cli.extend(["--wallet", self.options.wallet, "--submit"])
return cli

    def transfer(self, from: str, to: str, amount: int, memo: Optional[str]) -> List[str]:
"""Returns CLI args; expected result type: Any"""
args = [
                (from, "Address"),
                (to, "Address"),
                (amount, "u128"),
                (memo, "Option<String>")
            ]
return self._invoke_args("transfer", args)

    def balance_of(self, owner: str) -> List[str]:
"""Returns CLI args; expected result type: int"""
args = [
                (owner, "Address")
            ]
return self._invoke_args("balance_of", args)

    def get_metadata(self, ) -> List[str]:
"""Returns CLI args; expected result type: TokenMetadata"""
args = [
            ]
return self._invoke_args("get_metadata", args)

    def batch_transfer(self, recipients: List[str], amounts: List[int]) -> List[str]:
"""Returns CLI args; expected result type: List[Any]"""
args = [
                (recipients, "Vec<Address>"),
                (amounts, "Vec<u128>")
            ]
return self._invoke_args("batch_transfer", args)

    def set_config(self, key: str, value: bytes) -> List[str]:
"""Returns CLI args; expected result type: None"""
args = [
                (key, "Symbol"),
                (value, "Bytes")
            ]
return self._invoke_args("set_config", args)


@dataclass
class TokenMetadata:
    name: str
    symbol: str
    decimals: int
    total_supply: int
    admin: str

@dataclass
class Allowance:
    owner: str
    spender: str
    amount: int
    expires_at: Optional[int]

# Event type definitions
@dataclass
class TransferEvent:
    from: str
    to: str
    amount: int
