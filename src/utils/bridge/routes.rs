use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeRoute {
    pub id: String,
    pub source_network: String,
    pub dest_network: String,
    pub asset: String,
    pub provider: String,
    pub min_amount: u64,
    pub max_amount: u64,
    pub fee_bps: u16,
    pub estimated_time_secs: u64,
    pub enabled: bool,
}

pub struct RouteRegistry {
    routes: Vec<BridgeRoute>,
}

impl RouteRegistry {
    pub fn new(routes: Vec<BridgeRoute>) -> Self {
        Self { routes }
    }

    pub fn from_defaults() -> Self {
        Self::new(default_routes())
    }

    pub fn all(&self) -> &[BridgeRoute] {
        &self.routes
    }

    pub fn find(&self, source: &str, dest: &str, asset: Option<&str>) -> Vec<&BridgeRoute> {
        self.routes
            .iter()
            .filter(|r| {
                r.enabled
                    && r.source_network == source
                    && r.dest_network == dest
                    && asset.map_or(true, |a| r.asset == a)
            })
            .collect()
    }

    pub fn find_by_id(&self, id: &str) -> Option<&BridgeRoute> {
        self.routes.iter().find(|r| r.id == id)
    }

    pub fn best_route(&self, source: &str, dest: &str, asset: &str) -> Option<&BridgeRoute> {
        self.find(source, dest, Some(asset))
            .into_iter()
            .min_by_key(|r| r.fee_bps)
    }
}

pub fn default_routes() -> Vec<BridgeRoute> {
    vec![
        BridgeRoute {
            id: "stellar-testnet-to-eth-sepolia-usdc".to_string(),
            source_network: "stellar-testnet".to_string(),
            dest_network: "ethereum-sepolia".to_string(),
            asset: "USDC".to_string(),
            provider: "stellar-allbridge".to_string(),
            min_amount: 1_000_000,
            max_amount: 100_000_000_000,
            fee_bps: 30,
            estimated_time_secs: 180,
            enabled: true,
        },
        BridgeRoute {
            id: "stellar-testnet-to-polygon-amoy-usdc".to_string(),
            source_network: "stellar-testnet".to_string(),
            dest_network: "polygon-amoy".to_string(),
            asset: "USDC".to_string(),
            provider: "stellar-wormhole".to_string(),
            min_amount: 1_000_000,
            max_amount: 50_000_000_000,
            fee_bps: 25,
            estimated_time_secs: 240,
            enabled: true,
        },
        BridgeRoute {
            id: "eth-sepolia-to-stellar-testnet-usdc".to_string(),
            source_network: "ethereum-sepolia".to_string(),
            dest_network: "stellar-testnet".to_string(),
            asset: "USDC".to_string(),
            provider: "stellar-allbridge".to_string(),
            min_amount: 1_000_000,
            max_amount: 100_000_000_000,
            fee_bps: 30,
            estimated_time_secs: 200,
            enabled: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_routes_by_network_pair() {
        let registry = RouteRegistry::from_defaults();
        let routes = registry.find("stellar-testnet", "ethereum-sepolia", Some("USDC"));
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].provider, "stellar-allbridge");
    }

    #[test]
    fn best_route_picks_lowest_fee() {
        let registry = RouteRegistry::from_defaults();
        let route = registry
            .best_route("stellar-testnet", "ethereum-sepolia", "USDC")
            .unwrap();
        assert_eq!(route.fee_bps, 30);
    }
}
