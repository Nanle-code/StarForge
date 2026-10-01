use clap::Args;

#[derive(Args, Debug, Clone, Default)]
pub struct NetworkFlag {
    /// The network to connect to (e.g. testnet, mainnet, futurenet, standalone)
    #[arg(long, global = true, env = "STARFORGE_NETWORK")]
    pub network: Option<String>,
}

#[derive(Args, Debug, Clone, Default)]
pub struct WalletFlag {
    /// The wallet or source account to use
    #[arg(long, global = true, alias = "source", env = "STARFORGE_WALLET")]
    pub wallet: Option<String>,
}

#[derive(Args, Debug, Clone, Default)]
pub struct YesFlag {
    /// Skip confirmation prompts
    #[arg(long, global = true, alias = "force")]
    pub yes: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct VerboseFlag {
    /// Enable verbose output
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,
}
