use soroban_sdk::{contracttype, Address, Map, String};

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum Role {
    Issuer,
    Buyer,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum VerificationStatus {
    /// Address has no profile on record.
    Unregistered,
    /// Address has an admin-verified profile.
    Verified,
    /// Address has a profile that the admin has not (yet) verified, or has
    /// explicitly revoked. Newly self-registered profiles start here until
    /// an admin calls `verify_profile(&addr, &true)` (see issue #130).
    Unverified,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Profile {
    pub address: Address,
    pub role: Role,
    pub verified: bool,
    pub registered_at: u64,
    pub metadata: Map<String, String>,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Profile(Address),
}
