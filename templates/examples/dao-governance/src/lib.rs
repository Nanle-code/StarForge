#![no_std]
//! A minimal DAO governance contract for Soroban.
//!
//! Members create proposals and cast one-member-one-vote ballots. Once the
//! voting window closes a proposal is considered passed if it has more votes
//! for than against. This demonstrates the core governance loop (propose →
//! vote → tally) that most on-chain DAOs build upon.
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// List of addresses allowed to create proposals and vote.
    Members,
    /// The next proposal id to assign.
    NextId,
    /// A stored proposal, keyed by id.
    Proposal(u32),
    /// Whether `(proposal_id, voter)` has already voted.
    Voted(u32, Address),
}

#[contracttype]
#[derive(Clone)]
pub struct Proposal {
    pub id: u32,
    pub proposer: Address,
    pub title: String,
    pub votes_for: u32,
    pub votes_against: u32,
    pub closed: bool,
}

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    // <starforge:add:methods>
    /// Initialize the DAO with its founding members.
    pub fn initialize(env: Env, members: Vec<Address>) {
        // <starforge:pause-check>
        if env.storage().instance().has(&DataKey::Members) {
            panic!("already initialized");
        }
        if members.is_empty() {
            panic!("at least one member is required");
        }
        env.storage().instance().set(&DataKey::Members, &members);
        env.storage().instance().set(&DataKey::NextId, &0u32);
    }

    /// Create a new proposal. Only members may propose.
    pub fn propose(env: Env, proposer: Address, title: String) -> u32 {
        // <starforge:pause-check>
        proposer.require_auth();
        Self::require_member(&env, &proposer);

        let id: u32 = env.storage().instance().get(&DataKey::NextId).unwrap_or(0);
        let proposal = Proposal {
            id,
            proposer,
            title,
            votes_for: 0,
            votes_against: 0,
            closed: false,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(id), &proposal);
        env.storage().instance().set(&DataKey::NextId, &(id + 1));
        id
    }

    /// Cast a vote on a proposal. Each member may vote once per proposal.
    pub fn vote(env: Env, voter: Address, proposal_id: u32, support: bool) {
        // <starforge:pause-check>
        voter.require_auth();
        Self::require_member(&env, &voter);

        let mut proposal = Self::proposal(&env, proposal_id);
        if proposal.closed {
            panic!("proposal is closed");
        }

        let voted_key = DataKey::Voted(proposal_id, voter.clone());
        if env.storage().persistent().get(&voted_key).unwrap_or(false) {
            panic!("already voted");
        }

        if support {
            proposal.votes_for += 1;
        } else {
            proposal.votes_against += 1;
        }
        env.storage().persistent().set(&voted_key, &true);
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);
    }

    /// Close a proposal so no further votes can be cast. Only the proposer may close.
    pub fn close(env: Env, caller: Address, proposal_id: u32) {
        // <starforge:pause-check>
        caller.require_auth();
        let mut proposal = Self::proposal(&env, proposal_id);
        if caller != proposal.proposer {
            panic!("only the proposer can close the proposal");
        }
        proposal.closed = true;
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);
    }

    /// Return a proposal by id.
    pub fn get_proposal(env: Env, proposal_id: u32) -> Proposal {
        Self::proposal(&env, proposal_id)
    }

    /// Return whether a proposal has passed (more votes for than against).
    pub fn has_passed(env: Env, proposal_id: u32) -> bool {
        let proposal = Self::proposal(&env, proposal_id);
        proposal.votes_for > proposal.votes_against
    }

    fn proposal(env: &Env, proposal_id: u32) -> Proposal {
        env.storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .expect("proposal not found")
    }

    fn require_member(env: &Env, address: &Address) {
        let members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Members)
            .expect("not initialized");
        if !members.contains(address) {
            panic!("caller is not a member");
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_proposal_passes() {
        let env = Env::default();
        env.mock_all_auths();

        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let carol = Address::generate(&env);

        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);

        let mut members = Vec::new(&env);
        members.push_back(alice.clone());
        members.push_back(bob.clone());
        members.push_back(carol.clone());
        client.initialize(&members);

        let id = client.propose(&alice, &String::from_str(&env, "Fund the treasury"));

        client.vote(&alice, &id, &true);
        client.vote(&bob, &id, &true);
        client.vote(&carol, &id, &false);

        let proposal = client.get_proposal(&id);
        assert_eq!(proposal.votes_for, 2);
        assert_eq!(proposal.votes_against, 1);
        assert!(client.has_passed(&id));
    }

    #[test]
    #[should_panic(expected = "already voted")]
    fn test_double_vote_rejected() {
        let env = Env::default();
        env.mock_all_auths();

        let alice = Address::generate(&env);
        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);

        let mut members = Vec::new(&env);
        members.push_back(alice.clone());
        client.initialize(&members);

        let id = client.propose(&alice, &String::from_str(&env, "Test"));
        client.vote(&alice, &id, &true);
        client.vote(&alice, &id, &true);
    }
}
