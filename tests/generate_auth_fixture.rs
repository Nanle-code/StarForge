use stellar_xdr::curr::{
    Hash, ScAddress, ScSymbol, ScVal, SorobanAuthorizationEntry, SorobanAuthorizedFunction,
    SorobanAuthorizedInvocation, SorobanCredentials, VecM, WriteXdr, Limits,
};

#[test]
fn test_auth_tree_snapshot() {
    let mut args = VecM::default();
    args.push(ScVal::I32(42)).unwrap();
    let contract_address = ScAddress::Contract(Hash([0; 32]));

    let mut sub_invocations = VecM::default();
    sub_invocations
        .push(SorobanAuthorizedInvocation {
            function: SorobanAuthorizedFunction::ContractFn(
                stellar_xdr::curr::InvokeContractArgs {
                    contract_address: contract_address.clone(),
                    function_name: ScSymbol("transfer".try_into().unwrap()),
                    args: VecM::default(),
                },
            ),
            sub_invocations: VecM::default(),
        })
        .unwrap();

    let root_invocation = SorobanAuthorizedInvocation {
        function: SorobanAuthorizedFunction::ContractFn(stellar_xdr::curr::InvokeContractArgs {
            contract_address,
            function_name: ScSymbol("swap".try_into().unwrap()),
            args,
        }),
        sub_invocations,
    };

    let entry = SorobanAuthorizationEntry {
        credentials: SorobanCredentials::SorobanCredentialsAddress(
            stellar_xdr::curr::SorobanAddressCredentials {
                address: ScAddress::Contract(Hash([1; 32])),
                nonce: 1,
                signature_expiration_ledger: 100,
                signature: stellar_xdr::curr::ScVal::Void,
            },
        ),
        root_invocation,
    };

    let b64 = entry.to_xdr_base64(Limits::none()).unwrap();

    let json = serde_json::json!({
        "results": [{
            "auth": [b64]
        }],
        "events": [],
        "returnValue": "AAAAAA=="
    });
    
    // I would use build_simulation_result here but it's private.
    // However, I can test it through the JSON deserialize if I had it exposed,
    // or we just save this JSON to a fixture.
    
    let path = std::path::PathBuf::from("tests/fixtures/soroban_rpc/simulate_nested_auth.json");
    std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
}
