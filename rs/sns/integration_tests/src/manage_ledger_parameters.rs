use candid::{Decode, Encode};
use candid::{Nat, Principal};
use ic_base_types::{CanisterId, PrincipalId};
use ic_canister_client_sender::Sender;
use ic_ledger_core::Tokens;
use ic_nervous_system_common::ledger::compute_neuron_staking_subaccount;
use ic_nervous_system_common_test_keys::TEST_USER1_KEYPAIR;
use ic_nns_test_utils::{
    common::NnsInitPayloadsBuilder,
    sns_wasm::{add_wasm_via_proposal, build_ledger_sns_wasm},
    state_test_helpers::{
        icrc1_fee, icrc1_transfer, query, setup_nns_canisters, sns_claim_staked_neuron,
        sns_make_proposal, sns_wait_for_proposal_execution, update,
    },
};
use ic_sns_governance::pb::v1::{
    manage_ledger_parameters::ChangeFeeCollector, proposal::Action, Empty, ManageLedgerParameters,
    NervousSystemParameters, NeuronId, NeuronPermissionList, NeuronPermissionType, Proposal,
    ProposalId,
};
use ic_sns_test_utils::{
    itest_helpers::SnsTestsInitPayloadBuilder,
    state_test_helpers::{setup_sns_canisters, SnsTestCanisterIds},
};
use ic_state_machine_tests::StateMachine;
use icrc_ledger_types::icrc1::{account::Account, transfer::TransferArg};
use num_traits::cast::ToPrimitive;

const DEFAULT_LEDGER_TRANSFER_FEE: u64 = 10_000;
const DEFAULT_NEURON_STAKE: u64 = 500500000000;

#[test]
fn test_manage_ledger_parameters_change_transfer_fee() {
    let state_machine = StateMachine::new();

    let user = PrincipalId::new_user_test_id(1000);

    let (sns_canisters, neuron) = set_up_sns_for_mlp(&state_machine, &user);

    assert_eq!(
        icrc1_fee(&state_machine, sns_canisters.ledger_canister_id)
            .0
            .to_u64()
            .unwrap(),
        DEFAULT_LEDGER_TRANSFER_FEE
    );

    // change ledger transfer_fee with the ManageLedgerParameters proposal
    let new_fee = 34;

    let change_ledger_transfer_fee_proposal_id = sns_make_proposal(
        &state_machine,
        sns_canisters.governance_canister_id,
        user,
        neuron.clone(),
        Proposal {
            title: "Change ledger transfer fee".to_string(),
            action: Some(Action::ManageLedgerParameters(ManageLedgerParameters {
                transfer_fee: Some(new_fee),
                ..Default::default()
            })),
            ..Default::default()
        },
    )
    .unwrap();

    sns_wait_for_proposal_execution(
        &state_machine,
        sns_canisters.governance_canister_id,
        change_ledger_transfer_fee_proposal_id,
    );

    wait_for_ledger_canister_to_start_after_an_upgrade(
        &state_machine,
        sns_canisters.ledger_canister_id,
    );

    // check that the fee on the ledger has changed.
    let ledger_fee_after_proposal = icrc1_fee(&state_machine, sns_canisters.ledger_canister_id);

    assert!(ledger_fee_after_proposal.0.to_u64().unwrap() != DEFAULT_LEDGER_TRANSFER_FEE);
    assert!(ledger_fee_after_proposal.0.to_u64().unwrap() == new_fee);

    // try making transfers using the new fee and the old fee.
    icrc1_transfer(
        &state_machine,
        sns_canisters.ledger_canister_id,
        user,
        TransferArg {
            amount: Nat::from(5),
            fee: Some(Nat::from(new_fee)),
            from_subaccount: None,
            to: Account {
                owner: Principal::management_canister(),
                subaccount: None,
            },
            memo: None,
            created_at_time: None,
        },
    )
    .expect("This transfer with the new fee must succeed");

    icrc1_transfer(
        &state_machine,
        sns_canisters.ledger_canister_id,
        user,
        TransferArg {
            amount: Nat::from(5),
            fee: Some(DEFAULT_LEDGER_TRANSFER_FEE.into()),
            from_subaccount: None,
            to: Account {
                owner: Principal::management_canister(),
                subaccount: None,
            },
            memo: None,
            created_at_time: None,
        },
    )
    .expect_err("This transfer with the old fee must fail.");

    let nervous_system_parameters_with_new_fee: NervousSystemParameters = {
        let nervous_system_parameters_raw = query(
            &state_machine,
            sns_canisters.governance_canister_id,
            "get_nervous_system_parameters",
            Encode!().unwrap(),
        )
        .unwrap();

        candid::decode_one(&nervous_system_parameters_raw).unwrap()
    };

    assert_eq!(
        nervous_system_parameters_with_new_fee.transaction_fee_e8s,
        Some(new_fee)
    );
}

#[test]
fn test_manage_ledger_parameters_change_fee_collector() {
    let state_machine = StateMachine::new();

    let user = PrincipalId::new_user_test_id(1000);

    let (sns_canisters, neuron) = set_up_sns_for_mlp(&state_machine, &user);

    // choose a new fee_collector
    let new_fee_collector = Account {
        owner: Sender::from_keypair(&TEST_USER1_KEYPAIR)
            .get_principal_id()
            .0,
        subaccount: None,
    };

    // check that a transfer does not send the fee to the new_fee_collector before the proposal
    icrc1_transfer(
        &state_machine,
        sns_canisters.ledger_canister_id,
        user,
        TransferArg {
            amount: Nat::from(5),
            fee: Some(DEFAULT_LEDGER_TRANSFER_FEE.into()),
            from_subaccount: None,
            to: Account {
                owner: Principal::management_canister(),
                subaccount: None,
            },
            memo: None,
            created_at_time: None,
        },
    )
    .unwrap();

    assert_eq!(
        icrc1_balance(
            &state_machine,
            sns_canisters.ledger_canister_id,
            new_fee_collector
        ),
        Nat::from(0)
    );

    // change the sns-ledger's fee_collector with the ManageLedgerParameters proposal
    let change_fee_collector_proposal_id: ProposalId = sns_make_proposal(
        &state_machine,
        sns_canisters.governance_canister_id,
        user,
        neuron.clone(),
        Proposal {
            title: "ManageLedgerParameters".to_string(),
            action: Some(Action::ManageLedgerParameters(ManageLedgerParameters {
                change_fee_collector: Some(ChangeFeeCollector::SetTo(new_fee_collector.into())),
                ..Default::default()
            })),
            ..Default::default()
        },
    )
    .unwrap();

    sns_wait_for_proposal_execution(
        &state_machine,
        sns_canisters.governance_canister_id,
        change_fee_collector_proposal_id,
    );

    wait_for_ledger_canister_to_start_after_an_upgrade(
        &state_machine,
        sns_canisters.ledger_canister_id,
    );

    // check that a transfer does send the fee to the new fee_collector now.
    icrc1_transfer(
        &state_machine,
        sns_canisters.ledger_canister_id,
        user,
        TransferArg {
            amount: Nat::from(5),
            fee: Some(DEFAULT_LEDGER_TRANSFER_FEE.into()),
            from_subaccount: None,
            to: Account {
                owner: Principal::management_canister(),
                subaccount: None,
            },
            memo: None,
            created_at_time: None,
        },
    )
    .unwrap();

    assert_eq!(
        icrc1_balance(
            &state_machine,
            sns_canisters.ledger_canister_id,
            new_fee_collector
        ),
        Nat::from(DEFAULT_LEDGER_TRANSFER_FEE)
    );

    // Unset the sns-ledger's fee_collector
    let unset_fee_collector_proposal_id: ProposalId = sns_make_proposal(
        &state_machine,
        sns_canisters.governance_canister_id,
        user,
        neuron.clone(),
        Proposal {
            title: "ManageLedgerParameters".to_string(),
            action: Some(Action::ManageLedgerParameters(ManageLedgerParameters {
                change_fee_collector: Some(ChangeFeeCollector::Unset(Empty {})),
                ..Default::default()
            })),
            ..Default::default()
        },
    )
    .unwrap();

    sns_wait_for_proposal_execution(
        &state_machine,
        sns_canisters.governance_canister_id,
        unset_fee_collector_proposal_id,
    );

    wait_for_ledger_canister_to_start_after_an_upgrade(
        &state_machine,
        sns_canisters.ledger_canister_id,
    );

    // make sure a transfer-fee does not go to the previous fee-collector.
    let fee_collector_balance_before_unset = icrc1_balance(
        &state_machine,
        sns_canisters.ledger_canister_id,
        new_fee_collector,
    );

    icrc1_transfer(
        &state_machine,
        sns_canisters.ledger_canister_id,
        user,
        TransferArg {
            amount: Nat::from(5),
            fee: Some(DEFAULT_LEDGER_TRANSFER_FEE.into()),
            from_subaccount: None,
            to: Account {
                owner: Principal::management_canister(),
                subaccount: None,
            },
            memo: None,
            created_at_time: None,
        },
    )
    .unwrap();

    assert_eq!(
        icrc1_balance(
            &state_machine,
            sns_canisters.ledger_canister_id,
            new_fee_collector
        ),
        fee_collector_balance_before_unset
    );
}

fn wait_for_ledger_canister_to_start_after_an_upgrade(
    machine: &StateMachine,
    ledger_id: CanisterId,
) {
    for i in 0..=20 {
        match update(machine, ledger_id, "icrc1_fee", Encode!().unwrap()) {
            Ok(_) => break,
            Err(call_error) => println!(
                "call error when checking if ledger is running: {:?}",
                call_error
            ),
        }
        if i == 20 {
            panic!("cannot verify the ledger is running.");
        }
    }
}

pub fn icrc1_balance(machine: &StateMachine, ledger_id: CanisterId, account: Account) -> Nat {
    let result = query(
        machine,
        ledger_id,
        "icrc1_balance_of",
        Encode!(&account).unwrap(),
    )
    .unwrap();
    Decode!(&result, Nat).unwrap()
}

fn set_up_sns_for_mlp(
    state_machine: &StateMachine,
    user: &PrincipalId,
) -> (SnsTestCanisterIds, NeuronId) {
    let user_account = Account {
        owner: user.0,
        subaccount: None,
    };

    let nns_init_payloads = NnsInitPayloadsBuilder::new().with_test_neurons().build();

    let system_params = NervousSystemParameters {
        neuron_claimer_permissions: Some(NeuronPermissionList {
            permissions: NeuronPermissionType::all(),
        }),
        ..NervousSystemParameters::with_default_values()
    };

    let sns_init_payload = SnsTestsInitPayloadBuilder::new()
        .with_ledger_account(user_account, Tokens::new(10001, 0).unwrap())
        .with_nervous_system_parameters(system_params)
        .with_ledger_transfer_fee(DEFAULT_LEDGER_TRANSFER_FEE)
        .build();

    setup_nns_canisters(state_machine, nns_init_payloads);
    let sns_canisters = setup_sns_canisters(state_machine, sns_init_payload);

    add_wasm_via_proposal(state_machine, build_ledger_sns_wasm());

    let neuron_nonce = 0;
    icrc1_transfer(
        state_machine,
        sns_canisters.ledger_canister_id,
        *user,
        TransferArg {
            from_subaccount: None,
            to: Account {
                owner: sns_canisters.governance_canister_id.get().0,
                subaccount: Some(compute_neuron_staking_subaccount(*user, neuron_nonce).0),
            },
            fee: Some(Nat::from(DEFAULT_LEDGER_TRANSFER_FEE)),
            created_at_time: None,
            memo: None,
            amount: DEFAULT_NEURON_STAKE.into(),
        },
    )
    .unwrap();

    let neuron = sns_claim_staked_neuron(
        state_machine,
        sns_canisters.governance_canister_id,
        *user,
        neuron_nonce,
        Some(100_000_000), // dissolve delay
    );

    (sns_canisters, neuron)
}
