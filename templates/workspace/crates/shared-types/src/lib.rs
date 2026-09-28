#![no_std]
use soroban_sdk::{contracttype, Address, String};

#[derive(Clone)]
#[contracttype]
pub struct UserConfig {
    pub account: Address,
    pub name: String,
}
