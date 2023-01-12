use async_trait::async_trait;
use ic_base_types::CanisterId;
use ic_icrc1::{Account, Subaccount};
use ic_ledger_core::Tokens;
use ic_nervous_system_common::{ledger::ICRC1Ledger, NervousSystemError, E8};
use ic_sns_governance::pb::v1::{
    manage_neuron_response, manage_neuron_response::ClaimOrRefreshResponse,
    ClaimSwapNeuronsRequest, ClaimSwapNeuronsResponse, ManageNeuron, ManageNeuronResponse, SetMode,
    SetModeResponse,
};
use ic_sns_swap::{
    pb::v1::{
        CanisterCallError, GovernanceError, SetDappControllersRequest, SetDappControllersResponse,
        SettleCommunityFundParticipation,
    },
    swap::{NnsGovernanceClient, SnsGovernanceClient, SnsRootClient},
};
use std::sync::{Arc, Mutex};

/// Expect that no SNS root calls will be made. Explode otherwise.
#[derive(Default, Debug)]
pub struct ExplodingSnsRootClient;

#[async_trait]
impl SnsRootClient for ExplodingSnsRootClient {
    async fn set_dapp_controllers(
        &mut self,
        _request: SetDappControllersRequest,
    ) -> Result<SetDappControllersResponse, CanisterCallError> {
        unimplemented!();
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
pub enum SnsRootClientCall {
    SetDappControllers(SetDappControllersRequest),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
pub enum SnsRootClientReply {
    SetDappControllers(SetDappControllersResponse),
    CanisterCallError(CanisterCallError),
}

/// SnsRootClient that lets the test spy on the calls made
#[derive(Default, Debug)]
pub struct SpySnsRootClient {
    pub observed_calls: Vec<SnsRootClientCall>,
    pub replies: Vec<SnsRootClientReply>,
}

#[async_trait]
impl SnsRootClient for SpySnsRootClient {
    async fn set_dapp_controllers(
        &mut self,
        request: SetDappControllersRequest,
    ) -> Result<SetDappControllersResponse, CanisterCallError> {
        self.observed_calls
            .push(SnsRootClientCall::SetDappControllers(request));
        match self.replies.pop().unwrap() {
            SnsRootClientReply::SetDappControllers(reply) => Ok(reply),
            SnsRootClientReply::CanisterCallError(error) => Err(error),
        }
    }
}

impl SpySnsRootClient {
    pub fn new(replies: Vec<SnsRootClientReply>) -> Self {
        SpySnsRootClient {
            observed_calls: vec![],
            replies,
        }
    }

    pub fn push_reply(&mut self, reply: SnsRootClientReply) {
        self.replies.push(reply)
    }

    pub fn pop_observed_call(&mut self) -> SnsRootClientCall {
        self.observed_calls
            .pop()
            .expect("Expected there to be a call on SpySnsRootClient's observed_call stack")
    }
}

impl SnsRootClientReply {
    /// Useful function for creating an enum value with no failures.
    pub fn successful_set_dapp_controllers() -> Self {
        SnsRootClientReply::SetDappControllers(SetDappControllersResponse {
            failed_updates: vec![],
        })
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
pub enum SnsGovernanceClientCall {
    ClaimSwapNeurons(ClaimSwapNeuronsRequest),
    ManageNeuron(ManageNeuron),
    SetMode(SetMode),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
#[allow(unused)]
pub enum SnsGovernanceClientReply {
    ClaimSwapNeurons(ClaimSwapNeuronsResponse),
    ManageNeuron(ManageNeuronResponse),
    SetMode(SetModeResponse),
    CanisterCallError(CanisterCallError),
}

#[derive(Default, Debug)]
pub struct SpySnsGovernanceClient {
    pub calls: Vec<SnsGovernanceClientCall>,
    pub replies: Vec<SnsGovernanceClientReply>,
}

impl SpySnsGovernanceClient {
    pub fn new(replies: Vec<SnsGovernanceClientReply>) -> Self {
        SpySnsGovernanceClient {
            calls: vec![],
            replies,
        }
    }

    /// Use this method if the replies are irrelevant
    pub fn with_dummy_replies() -> Self {
        SpySnsGovernanceClient {
            calls: vec![],
            replies: vec![
                SnsGovernanceClientReply::ClaimSwapNeurons(ClaimSwapNeuronsResponse::default()),
                SnsGovernanceClientReply::ClaimSwapNeurons(ClaimSwapNeuronsResponse::default()),
            ],
        }
    }
}

#[async_trait]
impl SnsGovernanceClient for SpySnsGovernanceClient {
    async fn manage_neuron(
        &mut self,
        request: ManageNeuron,
    ) -> Result<ManageNeuronResponse, CanisterCallError> {
        self.calls
            .push(SnsGovernanceClientCall::ManageNeuron(request));
        Ok(ManageNeuronResponse {
            command: Some(manage_neuron_response::Command::ClaimOrRefresh(
                // Even an empty value can be used here, because it is not
                // actually used in this scenario (yet).
                ClaimOrRefreshResponse::default(),
            )),
        })
    }
    async fn set_mode(&mut self, request: SetMode) -> Result<SetModeResponse, CanisterCallError> {
        self.calls.push(SnsGovernanceClientCall::SetMode(request));
        Ok(SetModeResponse {})
    }

    async fn claim_swap_neurons(
        &mut self,
        request: ClaimSwapNeuronsRequest,
    ) -> Result<ClaimSwapNeuronsResponse, CanisterCallError> {
        self.calls
            .push(SnsGovernanceClientCall::ClaimSwapNeurons(request));
        match self.replies.pop().unwrap() {
            SnsGovernanceClientReply::ClaimSwapNeurons(reply) => Ok(reply),
            SnsGovernanceClientReply::CanisterCallError(error) => Err(error),
            unexpected_reply => panic!("Unexpected reply on the stack: {:?}", unexpected_reply),
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
pub enum NnsGovernanceClientCall {
    SettleCommunityFundParticipation(SettleCommunityFundParticipation),
}

/// NnsGovernanceClient that allows tests to spy on the calls made
#[derive(Default, Debug)]
pub struct SpyNnsGovernanceClient {
    pub calls: Vec<NnsGovernanceClientCall>,
}

#[async_trait]
impl NnsGovernanceClient for SpyNnsGovernanceClient {
    async fn settle_community_fund_participation(
        &mut self,
        request: SettleCommunityFundParticipation,
    ) -> Result<Result<(), GovernanceError>, CanisterCallError> {
        self.calls
            .push(NnsGovernanceClientCall::SettleCommunityFundParticipation(
                request,
            ));
        Ok(Ok(()))
    }
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub enum LedgerCall {
    TransferFunds {
        amount_e8s: u64,
        fee_e8s: u64,
        from_subaccount: Option<Subaccount>,
        to: Account,
        memo: u64,
    },

    AccountBalance {
        account_id: Account,
    },
}

/// Struct that allows tests to spy on the calls made
pub struct SpyLedger {
    calls: Arc<Mutex<Vec<LedgerCall>>>,
}
impl SpyLedger {
    pub fn new(calls: Arc<Mutex<Vec<LedgerCall>>>) -> Self {
        Self { calls }
    }
}

impl Default for SpyLedger {
    fn default() -> Self {
        SpyLedger::new(Arc::new(Mutex::new(Vec::<LedgerCall>::new())))
    }
}

#[async_trait]
impl ICRC1Ledger for SpyLedger {
    async fn transfer_funds(
        &self,
        amount_e8s: u64,
        fee_e8s: u64,
        from_subaccount: Option<Subaccount>,
        to: Account,
        memo: u64,
    ) -> Result</* block_height: */ u64, NervousSystemError> {
        self.calls.lock().unwrap().push(LedgerCall::TransferFunds {
            amount_e8s,
            fee_e8s,
            from_subaccount,
            to,
            memo,
        });

        Ok(42)
    }

    async fn total_supply(&self) -> Result<Tokens, NervousSystemError> {
        unimplemented!();
    }

    async fn account_balance(&self, account_id: Account) -> Result<Tokens, NervousSystemError> {
        self.calls
            .lock()
            .unwrap()
            .push(LedgerCall::AccountBalance { account_id });

        Ok(Tokens::from_e8s(10 * E8))
    }

    fn canister_id(&self) -> CanisterId {
        CanisterId::from_u64(1)
    }
}

/// Expectation of one call on the mock Ledger.
#[derive(Debug, Clone)]
pub enum LedgerExpect {
    AccountBalance(Account, Result<Tokens, i32>),
    TransferFunds(u64, u64, Option<Subaccount>, Account, u64, Result<u64, i32>),
}

pub struct MockLedger {
    pub expect: Arc<Mutex<Vec<LedgerExpect>>>,
}

impl MockLedger {
    fn pop(&self) -> Option<LedgerExpect> {
        (*self.expect).lock().unwrap().pop()
    }
}

#[async_trait]
impl ICRC1Ledger for MockLedger {
    async fn transfer_funds(
        &self,
        amount_e8s: u64,
        fee_e8s: u64,
        from_subaccount: Option<Subaccount>,
        to: Account,
        memo: u64,
    ) -> Result<u64, NervousSystemError> {
        match self.pop() {
            Some(LedgerExpect::TransferFunds(
                amount_e8s_,
                fee_e8s_,
                from_subaccount_,
                to_,
                memo_,
                result,
            )) => {
                assert_eq!(amount_e8s_, amount_e8s);
                assert_eq!(fee_e8s_, fee_e8s);
                assert_eq!(from_subaccount_, from_subaccount);
                assert_eq!(to_, to);
                assert_eq!(memo_, memo);
                return result.map_err(|x| NervousSystemError::new_with_message(format!("{}", x)));
            }
            x => panic!(
                "Received transfer_funds({}, {}, {:?}, {}, {}), expected {:?}",
                amount_e8s, fee_e8s, from_subaccount, to, memo, x
            ),
        }
    }

    async fn total_supply(&self) -> Result<Tokens, NervousSystemError> {
        unimplemented!()
    }

    async fn account_balance(&self, account: Account) -> Result<Tokens, NervousSystemError> {
        match self.pop() {
            Some(LedgerExpect::AccountBalance(account_, result)) => {
                assert_eq!(account_, account);
                return result.map_err(|x| NervousSystemError::new_with_message(format!("{}", x)));
            }
            x => panic!("Received account_balance({}), expected {:?}", account, x),
        }
    }

    fn canister_id(&self) -> CanisterId {
        CanisterId::from_u64(1)
    }
}