use crate::ckbtc::minter::utils::{
    assert_mint_transaction, assert_no_new_utxo, assert_no_transaction, ensure_wallet,
    generate_blocks, get_btc_address, get_btc_client, start_canister, stop_canister,
    upgrade_canister, wait_for_bitcoin_balance, BTC_BLOCK_SIZE,
};
use crate::{
    ckbtc::lib::{
        activate_ecdsa_signature, create_canister, install_bitcoin_canister, install_kyt,
        install_ledger, install_minter, set_kyt_api_key, subnet_sys, upgrade_kyt,
        BTC_MIN_CONFIRMATIONS, KYT_FEE, TEST_KEY_LOCAL,
    },
    driver::{
        test_env::TestEnv,
        test_env_api::{HasPublicApiUrl, IcNodeContainer},
    },
    util::{assert_create_agent, block_on, runtime_from_url, UniversalCanister},
};
use bitcoincore_rpc::RpcApi;
use candid::Principal;
use ic_base_types::PrincipalId;
use ic_ckbtc_agent::CkBtcMinterAgent;
use ic_ckbtc_kyt::KytMode;
use ic_ckbtc_minter::updates::get_withdrawal_account::compute_subaccount;
use ic_ckbtc_minter::updates::update_balance::{UpdateBalanceArgs, UpdateBalanceError, UtxoStatus};
use icrc_ledger_agent::Icrc1Agent;
use icrc_ledger_types::icrc1::account::Account;
use slog::debug;

/// Test update_balance method of the minter canister.
/// Verify proper state preservation after canister update.
/// Verify proper utxo management in case of a ledger failure during the mint operation.
pub fn test_kyt(env: TestEnv) {
    let logger = env.logger();
    let subnet_sys = subnet_sys(&env);
    let sys_node = subnet_sys.nodes().next().expect("No node in sys subnet.");

    // Get access to btc replica.
    let btc_rpc = get_btc_client(&env);

    // Create wallet if required.
    ensure_wallet(&btc_rpc, &logger);

    let default_btc_address = btc_rpc.get_new_address(None, None).unwrap();
    // Creating the 10 first block to reach the min confirmations of the minter canister.
    debug!(
        &logger,
        "Generating 10 blocks to default address: {}", &default_btc_address
    );
    btc_rpc
        .generate_to_address(10, &default_btc_address)
        .unwrap();

    block_on(async {
        let runtime = runtime_from_url(sys_node.get_public_url(), sys_node.effective_canister_id());
        install_bitcoin_canister(&runtime, &logger, &env).await;

        let mut ledger_canister = create_canister(&runtime).await;
        let mut minter_canister = create_canister(&runtime).await;
        let mut kyt_canister = create_canister(&runtime).await;

        let minting_user = minter_canister.canister_id().get();
        let agent = assert_create_agent(sys_node.get_public_url().as_str()).await;
        let agent_principal = agent.get_principal().unwrap();
        let kyt_id = install_kyt(
            &mut kyt_canister,
            &logger,
            &env,
            Principal::from(minting_user),
            vec![agent_principal],
        )
        .await;
        set_kyt_api_key(
            &agent,
            &kyt_id.get().0,
            agent_principal,
            "fake key".to_string(),
        )
        .await;
        let ledger_id = install_ledger(&env, &mut ledger_canister, minting_user, &logger).await;
        let minter_id =
            install_minter(&env, &mut minter_canister, ledger_id, &logger, 0, kyt_id).await;
        let minter = Principal::from(minter_id.get());

        let ledger = Principal::from(ledger_id.get());
        let universal_canister =
            UniversalCanister::new_with_retries(&agent, sys_node.effective_canister_id(), &logger)
                .await;
        activate_ecdsa_signature(sys_node, subnet_sys.subnet_id, TEST_KEY_LOCAL, &logger).await;

        let ledger_agent = Icrc1Agent {
            agent: agent.clone(),
            ledger_canister_id: ledger,
        };
        let minter_agent = CkBtcMinterAgent {
            agent: agent.clone(),
            minter_canister_id: minter,
        };

        let caller = agent
            .get_principal()
            .expect("Error while getting principal.");
        let subaccount0 = compute_subaccount(PrincipalId::from(caller), 0);
        let subaccount1 = compute_subaccount(PrincipalId::from(caller), 567);
        let account1 = Account {
            owner: caller,
            subaccount: Some(subaccount1),
        };

        // Get the BTC address of the caller's sub-accounts.
        let btc_address0 = get_btc_address(&minter_agent, &logger, subaccount0).await;
        let btc_address1 = get_btc_address(&minter_agent, &logger, subaccount1).await;

        // -- beginning of test logic --

        // We shouldn't have any new utxo for now.
        assert_no_new_utxo(&minter_agent, &subaccount0).await;
        assert_no_new_utxo(&minter_agent, &subaccount1).await;

        // Mint block to the first sub-account (with single utxo).
        generate_blocks(&btc_rpc, &logger, 1, &btc_address1);
        generate_blocks(&btc_rpc, &logger, BTC_MIN_CONFIRMATIONS, &btc_address0);

        // Put the kyt canister into reject all utxos mode.
        upgrade_kyt(&mut kyt_canister, KytMode::RejectAll).await;
        wait_for_bitcoin_balance(
            &universal_canister,
            &logger,
            BTC_MIN_CONFIRMATIONS as u64 * BTC_BLOCK_SIZE,
            &btc_address0,
        )
        .await;
        let update_balance_tainted_result = minter_agent
            .update_balance(UpdateBalanceArgs {
                owner: None,
                subaccount: Some(subaccount1),
            })
            .await
            .expect("Error while calling update_balance")
            .expect("expected to have at a valid result");
        assert_eq!(update_balance_tainted_result.len(), 1);

        if let UtxoStatus::Tainted(_) = &update_balance_tainted_result[0] {
        } else {
            panic!("expected the minter to see one tainted utxo");
        }
        assert_no_transaction(&ledger_agent, &logger).await;

        upgrade_canister(&mut minter_canister).await;
        // If the kyt canister is unavailable we should get an error.
        generate_blocks(&btc_rpc, &logger, 1, &btc_address1);
        generate_blocks(&btc_rpc, &logger, BTC_MIN_CONFIRMATIONS, &btc_address0);
        wait_for_bitcoin_balance(
            &universal_canister,
            &logger,
            2 * BTC_MIN_CONFIRMATIONS as u64 * BTC_BLOCK_SIZE,
            &btc_address0,
        )
        .await;

        stop_canister(&kyt_canister).await;
        let update_balance_kyt_unavailable = minter_agent
            .update_balance(UpdateBalanceArgs {
                owner: None,
                subaccount: Some(subaccount1),
            })
            .await
            .expect("Error while calling update_balance");
        match update_balance_kyt_unavailable {
            Err(UpdateBalanceError::TemporarilyUnavailable(_)) => (),
            other => {
                panic!(
                    "Expected the KYT canister to be unavailable, got {:?}",
                    other
                );
            }
        }
        start_canister(&kyt_canister).await;

        // Put the kyt canister into accept all utxos mode.
        upgrade_kyt(&mut kyt_canister, KytMode::AcceptAll).await;
        // Now that the kyt canister is available and accept all utxos
        // we should be able to mint new utxos.
        let update_balance_new_utxos = minter_agent
            .update_balance(UpdateBalanceArgs {
                owner: None,
                subaccount: Some(subaccount1),
            })
            .await
            .expect("Error while calling update_balance")
            .expect("Expected to have at least one utxo result.");
        assert_eq!(update_balance_new_utxos.len(), 1);

        if let UtxoStatus::Minted { block_index, .. } = &update_balance_new_utxos[0] {
            assert_mint_transaction(
                &ledger_agent,
                &logger,
                *block_index,
                &account1,
                BTC_BLOCK_SIZE - KYT_FEE,
            )
            .await;
        } else {
            panic!("expected the minter to see one not tainted utxo");
        }

        stop_canister(&ledger_canister).await;
        generate_blocks(&btc_rpc, &logger, 1, &btc_address1);
        generate_blocks(&btc_rpc, &logger, BTC_MIN_CONFIRMATIONS, &btc_address0);
        wait_for_bitcoin_balance(
            &universal_canister,
            &logger,
            3 * BTC_MIN_CONFIRMATIONS as u64 * BTC_BLOCK_SIZE,
            &btc_address0,
        )
        .await;
        let update_balance_new_utxos = minter_agent
            .update_balance(UpdateBalanceArgs {
                owner: None,
                subaccount: Some(subaccount1),
            })
            .await
            .expect("Error while calling update_balance")
            .expect("Expected to have at least one utxo result.");
        assert_eq!(update_balance_new_utxos.len(), 1);

        if let UtxoStatus::Checked(_) = &update_balance_new_utxos[0] {
        } else {
            panic!("Expected to have checked the utxos but not minted");
        }

        let metrics = minter_agent.get_metrics_map().await;
        let owed_kyt_amount = metrics
            .get(&"ckbtc_minter_owed_kyt_amount".to_string())
            .unwrap()
            .value;

        start_canister(&ledger_canister).await;
        let update_balance_new_utxos = minter_agent
            .update_balance(UpdateBalanceArgs {
                owner: None,
                subaccount: Some(subaccount1),
            })
            .await
            .expect("Error while calling update_balance")
            .expect("Expected to have at least one utxo result.");
        assert_eq!(update_balance_new_utxos.len(), 1);
        if let UtxoStatus::Minted { block_index, .. } = &update_balance_new_utxos[0] {
            assert_mint_transaction(
                &ledger_agent,
                &logger,
                *block_index,
                &account1,
                BTC_BLOCK_SIZE - KYT_FEE,
            )
            .await;
        } else {
            panic!("expected the minter to see one clean utxo");
        }
        let metrics = minter_agent.get_metrics_map().await;
        let owed_kyt_amount_after_update_balance = metrics
            .get(&"ckbtc_minter_owed_kyt_amount".to_string())
            .unwrap()
            .value;
        assert_eq!(owed_kyt_amount, owed_kyt_amount_after_update_balance);
    });
}