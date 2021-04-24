#![cfg_attr(not(feature = "std"), no_std)]

use ink_lang as ink;

#[ink::contract]
mod leasingmanager {
    use erc20::Erc20;
    use erc721::Erc721;

    use ink_env::call::FromAccountId;
    use ink_prelude::vec::Vec;
    use ink_storage::{
        collections::HashMap as StorageHashMap,
        traits::{PackedLayout, SpreadLayout, StorageLayout},
        Lazy,
    };
    use scale::{Decode, Encode};

    type TokenId = u32;
    type LeaseId = u64;
    #[derive(Encode, Decode, Debug, Default, Copy, Clone, SpreadLayout)]
    #[cfg_attr(feature = "std", derive(StorageLayout))]
    struct Ownable {
        owner: AccountId,
    }
    #[derive(Encode, Decode, Debug, Default, Copy, Clone, SpreadLayout)]
    #[cfg_attr(feature = "std", derive(StorageLayout))]
    pub struct Administration {
        enabled: bool,
    }

    #[derive(Encode, Decode, Debug, PartialEq, Eq, Copy, Clone)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum LeaseStatus {
        Available,
        Rented,
        Terminated,
    }

    #[derive(Encode, Decode, Debug, PartialEq, Eq, Copy, Clone)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum Error {
        LeasingNotEnabled,
        NoSuchLease,
        LeaseUnavailable,
        LeaseNotRented,
        NotInvestor,
        NotOwner,
        LeaseNotDefault,
        LeaseNotOver,
        ERC721TransferFailed,
        ERC20TransferFailed,
        InsufficientBalance,
    }

    #[derive(Clone, Default, Copy, Encode, Decode, Debug, SpreadLayout, PackedLayout)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo, StorageLayout))]
    pub struct Lease {
        id: LeaseId,
        token_id: TokenId,
        nft_address: AccountId,
        beneficiary_address: AccountId,
        daily_rent: u64,
        lease_duration: u64,
        investor_address: AccountId,
        renter_address: Option<AccountId>,
        created_at: u64,
        leased_at: Option<u64>,
        last_paid_at: Option<u64>,
        lease_paid_until: Option<u64>,
        terminated_at: Option<u64>,
        status: u8,
    }

    /// Defines the storage of your contract.
    /// Add new fields to the below struct in order
    /// to add new static storage fields to your contract.
    #[ink(storage)]
    pub struct LeasingManager {
        owner: Ownable,
        leases: StorageHashMap<LeaseId, Lease>,
        investors: StorageHashMap<AccountId, Vec<LeaseId>>,
        renters: StorageHashMap<AccountId, Vec<LeaseId>>,
        administration: Administration,
        total_leases: u32,
        erc20: Lazy<Erc20>,
        erc721: Lazy<Erc721>,
    }

    #[ink(event)]
    pub struct LeaseListed {
        #[ink(topic)]
        investor: AccountId,
        #[ink(topic)]
        nft_address: AccountId,
        #[ink(topic)]
        lease_id: LeaseId,
        token_id: u32,
        beneficiary_address: AccountId,
        daily_rent: Balance,
        lease_duration: u64,
    }

    #[ink(event)]
    pub struct LeaseAvailed {
        #[ink(topic)]
        renter: AccountId,
        #[ink(topic)]
        lease_id: LeaseId,
        #[ink(topic)]
        nft_address: AccountId,
        token_id: u32,
    }

    #[ink(event)]
    pub struct RentPaid {
        #[ink(topic)]
        renter: AccountId,
        #[ink(topic)]
        lease_id: LeaseId,
        #[ink(topic)]
        nft_address: AccountId,
        token_id: u32,
        rent_amount: Balance,
    }

    #[ink(event)]
    pub struct LeaseFinished {
        #[ink(topic)]
        investor: AccountId,
        #[ink(topic)]
        lease_id: LeaseId,
        #[ink(topic)]
        nft_address: AccountId,
        token_id: u32,
    }

    #[ink(event)]
    pub struct LeaseRemoved {
        #[ink(topic)]
        investor: AccountId,
        #[ink(topic)]
        lease_id: LeaseId,
        #[ink(topic)]
        nft_address: AccountId,
        token_id: u32,
    }

    #[ink(event)]
    pub struct Enabled {}

    #[ink(event)]
    pub struct Disbaled {}

    #[ink(event)]
    pub struct OwnershipTransferred {
        #[ink(topic)]
        from: AccountId,
        #[ink(topic)]
        to: AccountId,
    }

    pub const SECONDS_IN_DAYS: u64 = 86_400;

    impl LeasingManager {
        /// Constructors can delegate to other constructors.
        #[ink(constructor)]
        pub fn new(erc20_address: AccountId, erc721_address: AccountId, enabled: bool) -> Self {
            let owner = Self::env().caller();

            let erc20 = Erc20::from_account_id(erc20_address);
            let erc721 = Erc721::from_account_id(erc721_address);

            let instance = Self {
                owner: Ownable { owner },
                administration: Administration { enabled },
                leases: Default::default(),
                investors: Default::default(),
                renters: Default::default(),
                total_leases: 0,
                erc20: Lazy::new(erc20),
                erc721: Lazy::new(erc721),
            };
            instance
        }

        /// Checks if caller is owner of AssetManager contract
        #[ink(message)]
        pub fn is_owner(&self) -> bool {
            return self.env().caller() == self.owner.owner;
        }

        /// Gets owner address of AssetManager contract
        #[ink(message)]
        pub fn get_owner(&self) -> AccountId {
            self.owner.owner
        }

        /// Transfers ownership from current owner to new_owner address
        /// Can only be called by the current owner
        #[ink(message)]
        pub fn transfer_ownership(&mut self, new_owner: AccountId) -> bool {
            let caller = self.env().caller();
            assert!(self.only_owner(caller));
            self.owner.owner = new_owner;
            self.env().emit_event(OwnershipTransferred {
                from: caller,
                to: new_owner,
            });
            true
        }

        fn only_owner(&self, caller: AccountId) -> bool {
            caller == self.owner.owner
        }

        #[ink(message)]
        pub fn list_token(
            &mut self,
            nft_address: AccountId,
            token_id: TokenId,
            beneficiary_address: AccountId,
            daily_rent: u64,
            lease_duration: u64,
        ) -> Result<(), Error> {
            assert_eq!(self.is_enabled(), true, "Listing is not enabled");
            let caller = self.env().caller();
            let contract_address = self.env().account_id();
            // Transfer tokens from caller to contract
            let erc721_transfer = self
                .erc721
                .transfer_from(caller, contract_address, token_id);
            assert_eq!(
                erc721_transfer.is_ok(),
                true,
                "ERC721 Token transfer failed"
            );

            let lease_id = self.total_leases as LeaseId;
            // Add trade into current active list
            let lease = Lease {
                id: lease_id,
                daily_rent: daily_rent,
                nft_address: nft_address,
                token_id: token_id,
                investor_address: caller,
                beneficiary_address: beneficiary_address,
                renter_address: None,
                status: LeaseStatus::Available as u8,
                lease_duration: lease_duration,
                created_at: Self::get_current_time(),
                leased_at: None,
                last_paid_at: None,
                lease_paid_until: None,
                terminated_at: None,
            };
            self.leases.insert(lease_id, lease);
            self.total_leases += 1;

            let mut invested: Vec<LeaseId> = Vec::new();
            let investor_opt = self.investors.get_mut(&caller);
            if investor_opt.is_some() {
                invested = investor_opt.unwrap().to_vec();
            }
            invested.push(lease_id);

            self.investors.insert(caller, invested);
            Ok(())
        }

        #[ink(message)]
        pub fn rent(&mut self, lease_id: u64) -> Result<(), Error> {
            assert_eq!(self.is_enabled(), true, "Leasing is not enabled");
            let current_time = self.get_current_time();
            let caller = self.env().caller();

            let lease_opt = self.leases.get_mut(&lease_id);
            assert_eq!(lease_opt.is_some(), true, "No such lease found");

            let lease = lease_opt.unwrap();
            assert_eq!(
                lease.status,
                LeaseStatus::Available as u8,
                "Lease is not available"
            );

            // Transfer first day rent to beneficiary
            let erc20_transfer = self.erc20.transfer_from(
                caller,
                lease.beneficiary_address,
                lease.daily_rent as u128,
            );
            assert_eq!(erc20_transfer.is_ok(), true, "ERC20 Token transfer failed");

            // Mark lease as rented
            lease.renter_address = Some(caller);
            lease.leased_at = Some(current_time);
            lease.last_paid_at = Some(current_time);
            lease.status = LeaseStatus::Rented as u8;

            let mut rented: Vec<LeaseId> = Vec::new();
            let renter_opt = self.renters.get_mut(&caller);
            if renter_opt.is_some() {
                rented = renter_opt.unwrap().to_vec();
            }
            rented.push(lease_id);

            self.renters.insert(caller, rented);

            let lease_clone = lease.clone();
            self.env().emit_event(LeaseAvailed {
                renter: caller,
                nft_address: lease_clone.nft_address,
                lease_id: lease_clone.id,
                token_id: lease_clone.token_id,
            });

            Ok(())
        }

        #[ink(message)]
        pub fn pay_rent(&mut self, lease_id: u64) -> Result<(), Error> {
            let current_time = self.get_current_time();
            let caller = self.env().caller();

            let lease_opt = self.leases.get_mut(&lease_id);
            assert_eq!(lease_opt.is_some(), true, "No such lease found");

            let lease = lease_opt.unwrap();
            assert_eq!(
                lease.status,
                LeaseStatus::Rented as u8,
                "Lease is not rented"
            );

            // Transfer daily rent to beneficiary
            let erc20_transfer = self.erc20.transfer_from(
                caller,
                lease.beneficiary_address,
                lease.daily_rent as u128,
            );
            assert_eq!(erc20_transfer.is_ok(), true, "ERC20 Token transfer failed");

            lease.last_paid_at = Some(current_time);
            lease.status = LeaseStatus::Rented as u8;
            Ok(())
        }

        #[ink(message)]
        pub fn terminate(&mut self, lease_id: u64) -> Result<(), Error> {
            let caller = self.env().caller();

            let lease_opt = self.leases.get_mut(&lease_id);
            assert_eq!(lease_opt.is_some(), true, "No lease found");

            let lease = lease_opt.unwrap();
            if caller != lease.investor_address {
                return Err(Error::NotInvestor);
            }
            if LeaseStatus::Rented as u8 != lease.status {
                return Err(Error::LeaseNotRented);
            }

            if !Self::is_defaulter(lease) {
                return Err(Error::LeaseNotDefault);
            }

            if !Self::lease_duration_over(lease) {
                return Err(Error::LeaseNotOver);
            }

            // Transfer nft to investor
            let erc721_transfer = self.erc721.transfer(caller, lease.token_id);
            assert_eq!(
                erc721_transfer.is_ok(),
                true,
                "ERC721 Token transfer failed"
            );

            // Mark lease as terminated
            lease.status = LeaseStatus::Terminated as u8;

            let lease_clone = lease.clone();
            self.env().emit_event(LeaseTermintated {
                investor: caller,
                nft_address: lease_clone.nft_address,
                lease_id: lease_clone.id,
                token_id: lease_clone.token_id,
            });

            Ok(())
        }

        #[ink(message)]
        pub fn get_rented_assets(&self, renter: AccountId) -> Vec<LeaseId> {
            let renter_opt = self.renters.get(&renter);
            let mut rents: Vec<LeaseId> = Vec::new();

            if renter_opt.is_some() {
                rents = renter_opt.unwrap().to_vec();
            }
            rents
        }

        #[ink(message)]
        pub fn get_leased_assets(&self, investor: AccountId) -> Vec<LeaseId> {
            let investor_opt = self.investors.get(&investor);
            let mut leases: Vec<LeaseId> = Vec::new();

            if investor_opt.is_some() {
                leases = investor_opt.unwrap().to_vec();
            }
            leases
        }

        /// Allows owner to enable leasing
        #[ink(message)]
        pub fn enable(&mut self) {
            assert!(self.only_owner(self.env().caller()));
            self.administration.enabled = true;
            self.env().emit_event(Enabled {});
        }

        /// Allows owner to disable leasingleasingleasing
        #[ink(message)]
        pub fn disable(&mut self) {
            assert!(self.only_owner(self.env().caller()));
            self.administration.enabled = false;
            self.env().emit_event(Disbaled {});
        }

        /// Checks if leasing is enabled
        #[ink(message)]
        pub fn is_enabled(&self) -> bool {
            self.administration.enabled
        }

        fn get_current_time(&self) -> u64 {
            self.env().block_timestamp()
        }
    }

    /// Testcases
    #[cfg(test)]
    mod tests {
        /// Imports all the definitions from the outer scope so we can use them here.
        use super::*;
        use ink_lang as ink;
        /// We test if the constructor does its job.
        fn instantiate_erc20_contract() -> AccountId {
            let erc20 = Erc20::new(1000000);
            let callee =
                ink_env::account_id::<ink_env::DefaultEnvironment>().unwrap_or([0x0; 32].into());
            callee
        }
        fn instantiate_erc721_contract() -> AccountId {
            let erc20 = Erc721::new();
            let callee =
                ink_env::account_id::<ink_env::DefaultEnvironment>().unwrap_or([0x0; 32].into());
            callee
        }
        #[ink::test]
        fn new_works() {
            let leasingmanager = LeasingManager::new(
                instantiate_erc20_contract(),
                instantiate_erc721_contract(),
                true,
            );
            assert_eq!(leasingmanager.is_enabled(), true);
        }

        #[ink::test]
        fn enable_works() {
            let mut leasingmanager = LeasingManager::new(
                instantiate_erc20_contract(),
                instantiate_erc721_contract(),
                false,
            );
            assert_eq!(leasingmanager.is_enabled(), false);

            leasingmanager.enable();
            assert_eq!(leasingmanager.is_enabled(), true);
        }

        #[ink::test]
        fn disable_works() {
            let mut leasingmanager = LeasingManager::new(
                instantiate_erc20_contract(),
                instantiate_erc721_contract(),
                true,
            );
            assert_eq!(leasingmanager.is_enabled(), true);

            leasingmanager.disable();
            assert_eq!(leasingmanager.is_enabled(), false);
        }

        #[ink::test]
        fn lease_duration_works() {
            assert_eq!(
                LeasingManager::duration_in_days(SECONDS_IN_DAYS * 1 * 1000, 0),
                1
            );

            assert_eq!(
                LeasingManager::duration_in_days(
                    SECONDS_IN_DAYS * 3 * 1000,
                    SECONDS_IN_DAYS * 1 * 1000
                ),
                2
            );

            assert_eq!(
                LeasingManager::duration_in_days(
                    SECONDS_IN_DAYS * 3 * 1000,
                    (SECONDS_IN_DAYS + 1) * 1 * 1000
                ),
                2
            );

            assert_eq!(
                LeasingManager::duration_in_days(
                    SECONDS_IN_DAYS * 300 * 1000,
                    SECONDS_IN_DAYS * 1 * 1000
                ),
                299
            );

            assert_eq!(
                LeasingManager::duration_in_days(
                    (SECONDS_IN_DAYS + 1) * 1 * 1000,
                    SECONDS_IN_DAYS * 1 * 1000
                ),
                1
            );

            assert_eq!(
                LeasingManager::duration_in_days(
                    SECONDS_IN_DAYS * 1000 * 1000,
                    (SECONDS_IN_DAYS - 1) * 999 * 1000
                ),
                2
            );
        }
    }
}
