use std::collections::{HashMap, HashSet};

use candid::{Encode, Nat, Principal};
use ic_base_types::{CanisterId, PrincipalId};
use ic_icrc1::{endpoints::TransferArg, Account};
use ic_icrc1_ledger::{InitArgs as Icrc1InitArgs, LedgerArgument};
use ic_ledger_canister_core::archive::ArchiveOptions;
use ic_nervous_system_common::{E8, SECONDS_PER_DAY};

use ic_nns_test_utils::state_test_helpers::icrc1_transfer;
use ic_sns_swap::{
    pb::v1::{
        get_open_ticket_response, new_sale_ticket_response,
        params::NeuronBasketConstructionParameters, BuyerState, GetLifecycleResponse, Init,
        Lifecycle, NewSaleTicketResponse, OpenResponse, Params, RefreshBuyerTokensResponse, Ticket,
    },
    swap::principal_to_subaccount,
};

use ic_sns_test_utils::state_test_helpers::{
    get_buyer_state, get_lifecycle, get_open_ticket, get_sns_sale_parameters, new_sale_ticket,
    notify_payment_failure, open_sale, refresh_buyer_token,
};
use ic_state_machine_tests::StateMachine;
use icp_ledger::{
    AccountIdentifier, LedgerCanisterInitPayload as IcpInitArgs, DEFAULT_TRANSFER_FEE,
};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref DEFAULT_MINTING_ACCOUNT: Account = Account {
        owner: PrincipalId::new_user_test_id(1000),
        subaccount: None,
    };
    pub static ref DEFAULT_INITIAL_BALANCE: u64 = 10_000_000;
    pub static ref DEFAULT_ICP_LEDGER_CANISTER_ID: CanisterId = CanisterId::from_u64(0);
    pub static ref DEFAULT_ICRC1_LEDGER_CANISTER_ID: CanisterId = CanisterId::from_u64(1);
    pub static ref DEFAULT_SNS_SALE_CANISTER_ID: CanisterId = CanisterId::from_u64(2);
    pub static ref DEFAULT_NNS_GOVERNANCE_PRINCIPAL: Principal = Principal::anonymous();
    pub static ref DEFAULT_SNS_GOVERNANCE_PRINCIPAL: Principal = Principal::anonymous();
    pub static ref DEFAULT_SNS_ROOT_PRINCIPAL: Principal = Principal::anonymous();
    pub static ref DEFAULT_FALLBACK_CONTROLLER_PRINCIPAL_IDS: Vec<Principal> =
        vec![Principal::anonymous()];
    pub static ref DEFAULT_NEURON_MINIMUM_STAKE: u64 = 1_000_000;
    pub static ref DEFAULT_SNS_SALE_PARAMS: Params = Params {
        min_participants: 1,
        min_icp_e8s: 1,
        max_icp_e8s: 10_000_000,
        min_participant_icp_e8s: 1_010_000,
        max_participant_icp_e8s: 10_000_000,
        swap_due_timestamp_seconds: StateMachine::new()
            .time()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 13 * SECONDS_PER_DAY,
        sns_token_e8s: 10_000_000,
        neuron_basket_construction_parameters: Some(NeuronBasketConstructionParameters {
            count: 1,
            dissolve_delay_interval_seconds: 1,
        }),
        sale_delay_seconds: None,
    };
    pub static ref DEFAULT_ICRC1_ARCHIVE_OPTIONS: ArchiveOptions = ArchiveOptions {
        trigger_threshold: 1,
        num_blocks_to_archive: 1,
        node_max_memory_size_bytes: None,
        max_message_size_bytes: None,
        controller_id: PrincipalId::new_anonymous(),
        cycles_for_archive_creation: None,
        max_transactions_per_response: None,
    };
}

pub struct PaymentProtocolTestSetup {
    pub state_machine: StateMachine,
    pub sns_sale_canister_id: CanisterId,
    pub icp_ledger_canister_id: CanisterId,
    pub sns_ledger_canister_id: CanisterId,
    pub icp_ledger_minting_account: Account,
}

impl PaymentProtocolTestSetup {
    /// If no specific initialization arguments need to be used for a test, the default versions can be used by parsing None
    /// for all init args.
    pub fn default_setup() -> Self {
        let state_machine = StateMachine::new();
        let icp_ledger_id = state_machine.create_canister(None);
        let sns_ledger_id = state_machine.create_canister(None);
        let swap_id = state_machine.create_canister(None);

        //Make sure the created canisters all have the correct ID
        assert!(icp_ledger_id == *DEFAULT_ICP_LEDGER_CANISTER_ID);
        assert!(sns_ledger_id == *DEFAULT_ICRC1_LEDGER_CANISTER_ID);
        assert!(swap_id == *DEFAULT_SNS_SALE_CANISTER_ID);

        // install the ICP ledger
        {
            let wasm = ic_test_utilities_load_wasm::load_wasm(
                "../../rosetta-api/icp_ledger/ledger",
                "ledger-canister",
                &[],
            );
            let args = Encode!(&PaymentProtocolTestSetup::default_icp_init_args()).unwrap();
            state_machine
                .install_existing_canister(icp_ledger_id, wasm, args)
                .unwrap();
        }
        // install the sns ledger
        {
            let wasm = ic_test_utilities_load_wasm::load_wasm(
                "../../rosetta-api/icrc1/ledger",
                "ic-icrc1-ledger",
                &[],
            );
            let args = Encode!(&LedgerArgument::Init(
                PaymentProtocolTestSetup::default_icrc1_init_args()
            ))
            .unwrap();
            state_machine
                .install_existing_canister(sns_ledger_id, wasm, args)
                .unwrap();
        }

        // install the sale canister
        {
            let wasm = ic_test_utilities_load_wasm::load_wasm("../swap", "sns-swap-canister", &[]);
            let args = Encode!(&PaymentProtocolTestSetup::default_sns_sale_init_args()).unwrap();

            state_machine
                .install_existing_canister(swap_id, wasm, args)
                .unwrap();
        }

        Self {
            state_machine,
            sns_sale_canister_id: swap_id,
            icp_ledger_canister_id: icp_ledger_id,
            sns_ledger_canister_id: sns_ledger_id,
            icp_ledger_minting_account: *DEFAULT_MINTING_ACCOUNT,
        }
    }

    pub fn default_icp_init_args() -> IcpInitArgs {
        IcpInitArgs {
            minting_account: AccountIdentifier::from(*DEFAULT_MINTING_ACCOUNT),
            icrc1_minting_account: Some(*DEFAULT_MINTING_ACCOUNT),
            initial_values: HashMap::new(),
            max_message_size_bytes: None,
            transaction_window: None,
            archive_options: None,
            send_whitelist: HashSet::new(),
            transfer_fee: Some(DEFAULT_TRANSFER_FEE),
            token_symbol: Some("ICP".to_string()),
            token_name: Some("Internet Computer".to_string()),
        }
    }
    pub fn default_icrc1_init_args() -> Icrc1InitArgs {
        Icrc1InitArgs {
            minting_account: *DEFAULT_MINTING_ACCOUNT,
            initial_balances: vec![(
                Account {
                    owner: PrincipalId::from(*DEFAULT_SNS_SALE_CANISTER_ID),
                    subaccount: None,
                },
                *DEFAULT_INITIAL_BALANCE,
            )],
            transfer_fee: DEFAULT_TRANSFER_FEE.get_e8s(),
            token_name: "SNS Token".to_string(),
            token_symbol: "STK".to_string(),
            metadata: vec![],
            archive_options: DEFAULT_ICRC1_ARCHIVE_OPTIONS.clone(),
        }
    }

    pub fn default_sns_sale_init_args() -> Init {
        Init {
            nns_governance_canister_id: (*DEFAULT_NNS_GOVERNANCE_PRINCIPAL).to_string(),
            sns_governance_canister_id: (*DEFAULT_SNS_GOVERNANCE_PRINCIPAL).to_string(),
            sns_ledger_canister_id: (*DEFAULT_ICRC1_LEDGER_CANISTER_ID).to_string(),
            icp_ledger_canister_id: (*DEFAULT_ICP_LEDGER_CANISTER_ID).to_string(),
            sns_root_canister_id: (*DEFAULT_SNS_ROOT_PRINCIPAL).to_string(),
            fallback_controller_principal_ids: DEFAULT_FALLBACK_CONTROLLER_PRINCIPAL_IDS
                .clone()
                .into_iter()
                .map(|x| x.to_string())
                .collect(),
            transaction_fee_e8s: Some(DEFAULT_TRANSFER_FEE.get_e8s()),
            neuron_minimum_stake_e8s: Some(*DEFAULT_NEURON_MINIMUM_STAKE),
        }
    }

    pub fn default_params() -> Params {
        DEFAULT_SNS_SALE_PARAMS.clone()
    }

    pub fn mint_icp(&self, account: &Account, amount: &u64) -> Result<u64, String> {
        icrc1_transfer(
            &self.state_machine,
            self.icp_ledger_canister_id,
            self.icp_ledger_minting_account.owner,
            TransferArg {
                from_subaccount: None,
                to: *account,
                fee: None,
                created_at_time: None,
                memo: None,
                amount: Nat::from(*amount),
            },
        )
    }

    pub fn buy_sns_token(&self, sender: &PrincipalId, amount: &u64) -> Result<u64, String> {
        icrc1_transfer(
            &self.state_machine,
            self.icp_ledger_canister_id,
            *sender,
            TransferArg {
                from_subaccount: None,
                to: Account {
                    owner: self.sns_sale_canister_id.into(),
                    subaccount: Some(principal_to_subaccount(sender)),
                },
                fee: Some(Nat::from(DEFAULT_TRANSFER_FEE.get_e8s())),
                created_at_time: None,
                memo: None,
                amount: Nat::from(*amount),
            },
        )
    }

    pub fn open_sale(&self, params: Params) -> OpenResponse {
        open_sale(
            &self.state_machine,
            &self.sns_sale_canister_id,
            Some(params),
        )
    }

    pub fn get_buyer_state(&self, buyer: &PrincipalId) -> Option<BuyerState> {
        get_buyer_state(&self.state_machine, &self.sns_sale_canister_id, buyer).buyer_state
    }

    pub fn get_sns_sale_parameters(&self) -> Params {
        get_sns_sale_parameters(&self.state_machine, &self.sns_sale_canister_id)
            .params
            .unwrap()
    }

    pub fn refresh_buyer_token(
        &self,
        buyer: &PrincipalId,
    ) -> Result<RefreshBuyerTokensResponse, String> {
        refresh_buyer_token(&self.state_machine, &self.sns_sale_canister_id, buyer)
    }

    pub fn get_lifecycle(&self) -> GetLifecycleResponse {
        get_lifecycle(&self.state_machine, &self.sns_sale_canister_id)
    }

    pub fn get_open_ticket(
        &self,
        buyer: &PrincipalId,
    ) -> Result<Option<Ticket>, ic_sns_swap::pb::v1::get_open_ticket_response::Err> {
        match get_open_ticket(&self.state_machine, self.sns_sale_canister_id, *buyer).result {
            Some(res) => match res {
                ic_sns_swap::pb::v1::get_open_ticket_response::Result::Ok(ok) => Ok(ok.ticket),
                ic_sns_swap::pb::v1::get_open_ticket_response::Result::Err(err) => Err(err),
            },
            None => panic!("Get open ticket returned None"),
        }
    }

    pub fn new_sale_ticket(
        &self,
        buyer: &PrincipalId,
        amount_icp_e8s: &u64,
        subaccount: Option<Vec<u8>>,
    ) -> Result<Ticket, new_sale_ticket_response::Err> {
        new_sale_ticket(
            &self.state_machine,
            self.sns_sale_canister_id,
            *buyer,
            *amount_icp_e8s,
            subaccount,
        )
    }
    pub fn notify_payment_failure(&self, sender: &PrincipalId) -> Option<Ticket> {
        notify_payment_failure(&self.state_machine, &self.sns_sale_canister_id, sender).ticket
    }
}

#[test]
fn test_payment_flow_disabled_when_sale_not_open() {
    let user0 = PrincipalId::new_user_test_id(0);
    let payment_flow_protocol = PaymentProtocolTestSetup::default_setup();
    //Sale is not yet open --> Should not be able to call new_sale_ticket successfully
    assert!(payment_flow_protocol
        .new_sale_ticket(&user0, &E8, None)
        .is_err());

    // Sale is not yet open --> Should not be able to call get_open_ticket successfully
    assert!(payment_flow_protocol.get_open_ticket(&user0).is_err());
}

#[test]
fn test_get_open_ticket() {
    let user0 = PrincipalId::new_user_test_id(0);
    let payment_flow_protocol = PaymentProtocolTestSetup::default_setup();
    assert_eq!(
        payment_flow_protocol.get_open_ticket(&user0).unwrap_err(),
        get_open_ticket_response::Err {
            error_type: Some(get_open_ticket_response::err::Type::SaleNotOpen.into()),
        }
    );

    // open the sale
    payment_flow_protocol.open_sale(PaymentProtocolTestSetup::default_params());
    // get_open_ticket should return none when the sale is open but there are no tickets
    assert_eq!(payment_flow_protocol.get_open_ticket(&user0).unwrap(), None);
}

#[test]
fn test_new_sale_ticket() {
    let user0 = PrincipalId::new_user_test_id(0);
    let user1 = PrincipalId::new_user_test_id(1);
    let payment_flow_protocol = PaymentProtocolTestSetup::default_setup();
    payment_flow_protocol.open_sale(PaymentProtocolTestSetup::default_params());
    let params = payment_flow_protocol.get_sns_sale_parameters();
    // error when caller is anonymous
    assert_eq!(
        NewSaleTicketResponse {
            result: Some(ic_sns_swap::pb::v1::new_sale_ticket_response::Result::Err(
                payment_flow_protocol
                    .new_sale_ticket(
                        &PrincipalId::new_anonymous(),
                        &params.min_participant_icp_e8s,
                        None
                    )
                    .unwrap_err()
            ))
        },
        NewSaleTicketResponse::err_invalid_principal()
    );

    // error when subaccount is not 32 bytes
    assert_eq!(
        NewSaleTicketResponse {
            result: Some(ic_sns_swap::pb::v1::new_sale_ticket_response::Result::Err(
                payment_flow_protocol
                    .new_sale_ticket(&user0, &params.min_participant_icp_e8s, Some(vec![0; 31]))
                    .unwrap_err()
            ))
        },
        NewSaleTicketResponse::err_invalid_subaccount()
    );
    assert_eq!(
        NewSaleTicketResponse {
            result: Some(ic_sns_swap::pb::v1::new_sale_ticket_response::Result::Err(
                payment_flow_protocol
                    .new_sale_ticket(&user0, &params.min_participant_icp_e8s, Some(vec![0; 33]))
                    .unwrap_err()
            ))
        },
        NewSaleTicketResponse::err_invalid_subaccount()
    );

    // error when amount < min_participant_icp_e8s
    let res =
        payment_flow_protocol.new_sale_ticket(&user0, &(params.min_participant_icp_e8s - 1), None);
    let expected = NewSaleTicketResponse::err_invalid_user_amount(
        params.min_participant_icp_e8s,
        params.max_participant_icp_e8s,
    );
    assert_eq!(
        NewSaleTicketResponse {
            result: Some(new_sale_ticket_response::Result::Err(res.unwrap_err()))
        },
        expected
    );

    // error when amount > max_participant_icp_e8s
    let res =
        payment_flow_protocol.new_sale_ticket(&user0, &(params.max_participant_icp_e8s + 1), None);
    let expected = NewSaleTicketResponse::err_invalid_user_amount(
        params.min_participant_icp_e8s,
        params.max_participant_icp_e8s,
    );
    assert_eq!(
        NewSaleTicketResponse {
            result: Some(new_sale_ticket_response::Result::Err(res.unwrap_err()))
        },
        expected
    );

    // ticket correctly created
    let ticket = payment_flow_protocol
        .new_sale_ticket(&user0, &(params.min_participant_icp_e8s), None)
        .unwrap();

    //Ticket id counter starts with 0
    assert!(ticket.ticket_id == 0);

    // ticket can be retrieved
    let ticket_0 = payment_flow_protocol
        .get_open_ticket(&user0)
        .unwrap()
        .unwrap();

    // Make sure a new ticket can be created after the prior ticket was deleted
    let res =
        payment_flow_protocol.new_sale_ticket(&user0, &(params.min_participant_icp_e8s + 1), None);
    assert_eq!(
        NewSaleTicketResponse {
            result: Some(new_sale_ticket_response::Result::Err(res.unwrap_err()))
        },
        NewSaleTicketResponse::err_ticket_exists(ticket)
    );

    // ticket is still the same as before the error
    assert_eq!(
        payment_flow_protocol
            .get_open_ticket(&user0)
            .unwrap()
            .unwrap(),
        ticket_0
    );

    // Create new ticket for other user
    let ticket = payment_flow_protocol
        .new_sale_ticket(&user1, &(params.min_participant_icp_e8s), None)
        .unwrap();
    //Ticket id counter should now be at 1
    assert!(ticket.ticket_id == 1);

    // Make sure the ticket form user1 has an incremented ticket id
    let ticket_1 = payment_flow_protocol
        .get_open_ticket(&user1)
        .unwrap()
        .unwrap();
    assert!(ticket_1.ticket_id > ticket_0.ticket_id);

    //Test manual deleting ticket
    {
        // Make sure that there exists not ticket for the user0
        let deleted_ticket = payment_flow_protocol.notify_payment_failure(&user0);
        assert!(deleted_ticket.clone().unwrap().ticket_id == ticket_0.ticket_id);
        assert!(deleted_ticket.unwrap().ticket_id != ticket_1.ticket_id);
        let no_ticket_found = payment_flow_protocol.notify_payment_failure(&user0);
        assert!(no_ticket_found.is_none());

        // Make sure that there exists not ticket for the user1
        let deleted_ticket = payment_flow_protocol.notify_payment_failure(&user1);
        assert!(deleted_ticket.clone().unwrap().ticket_id == ticket_1.ticket_id);
        assert!(deleted_ticket.unwrap().ticket_id != ticket_0.ticket_id);
        let no_ticket_found = payment_flow_protocol.notify_payment_failure(&user1);
        assert!(no_ticket_found.is_none());
    }
}

#[test]
fn test_simple_refresh_buyer_token() {
    let user0 = PrincipalId::new_user_test_id(0);
    let payment_flow_protocol = PaymentProtocolTestSetup::default_setup();

    //Lifecycle of Swap should be Pending
    assert_eq!(
        payment_flow_protocol.get_lifecycle().lifecycle,
        Some(Lifecycle::Pending as i32)
    );

    payment_flow_protocol.open_sale(PaymentProtocolTestSetup::default_params());
    let params = payment_flow_protocol.get_sns_sale_parameters();

    //Get user0 some funds to participate in the sale
    assert!(payment_flow_protocol
        .mint_icp(&Account::from(user0), &(100 * E8))
        .is_ok());

    //Buy some tokens
    let amount0_0 = params.min_participant_icp_e8s;
    assert!(payment_flow_protocol
        .buy_sns_token(&user0, &amount0_0)
        .is_ok());

    //Get a ticket
    assert!(payment_flow_protocol
        .new_sale_ticket(&user0, &amount0_0, None)
        .is_ok());

    //Commit to the amount
    assert!(payment_flow_protocol.refresh_buyer_token(&user0).is_ok());

    //Check that the buyer state was updated accordingly
    assert_eq!(
        payment_flow_protocol
            .get_buyer_state(&user0)
            .unwrap()
            .icp
            .unwrap()
            .amount_e8s,
        amount0_0.clone()
    );
}