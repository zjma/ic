use ic_base_types::PrincipalId;
use ic_nervous_system_common::ONE_MONTH_SECONDS;
use ic_nervous_system_integration_tests::{
    create_service_nervous_system_builder::CreateServiceNervousSystemBuilder,
    pocket_ic_helpers,
    pocket_ic_helpers::{
        await_with_timeout, hash_sns_wasms, nns, sns,
        sns::governance::{
            EXPECTED_UPGRADE_DURATION_MAX_SECONDS, EXPECTED_UPGRADE_STEPS_REFRESH_MAX_SECONDS,
        },
    },
};
use ic_sns_governance::governance::UPGRADE_STEPS_INTERVAL_REFRESH_BACKOFF_SECONDS;
use ic_sns_swap::pb::v1::Lifecycle;
use ic_sns_wasm::pb::v1::SnsCanisterType;

pub async fn test_sns_upgrade(sns_canisters_to_upgrade: Vec<SnsCanisterType>) {
    let (pocket_ic, initial_sns_version) =
        pocket_ic_helpers::pocket_ic_for_sns_tests_with_mainnet_versions().await;

    eprintln!("Creating SNS ...");
    let create_service_nervous_system = CreateServiceNervousSystemBuilder::default()
        .with_governance_parameters_neuron_minimum_dissolve_delay_to_vote(ONE_MONTH_SECONDS * 6)
        .with_one_developer_neuron(
            PrincipalId::new_user_test_id(830947),
            ONE_MONTH_SECONDS * 6,
            756575,
            0,
        )
        .build();
    let swap_parameters = create_service_nervous_system
        .swap_parameters
        .clone()
        .unwrap();

    eprintln!("Deploying an SNS instance via proposal ...");
    let sns_instance_label = "1";
    let (sns, _) = nns::governance::propose_to_deploy_sns_and_wait(
        &pocket_ic,
        create_service_nervous_system,
        sns_instance_label,
    )
    .await;

    // Only spawn an archive if we're testing it
    if sns_canisters_to_upgrade.contains(&SnsCanisterType::Archive) {
        eprintln!("Testing if the Archive canister is spawned ...");
        sns::ensure_archive_canister_is_spawned_or_panic(
            &pocket_ic,
            sns.governance.canister_id,
            sns.ledger.canister_id,
        )
        .await;
    }

    eprintln!("Await the swap lifecycle ...");
    sns::swap::await_swap_lifecycle(&pocket_ic, sns.swap.canister_id, Lifecycle::Open)
        .await
        .unwrap();

    eprintln!("smoke_test_participate_and_finalize ...");
    sns::swap::smoke_test_participate_and_finalize(
        &pocket_ic,
        sns.swap.canister_id,
        swap_parameters,
    )
    .await;

    let mut latest_sns_version = initial_sns_version;

    for upgrade_pass in 0..2 {
        eprintln!("Upgrade pass {}", upgrade_pass);

        eprintln!("Adding all WASMs ...");
        for canister_type in &sns_canisters_to_upgrade {
            eprintln!("modify_and_add_wasm for {:?} ...", canister_type);
            latest_sns_version = nns::sns_wasm::modify_and_add_wasm(
                &pocket_ic,
                latest_sns_version,
                *canister_type,
                upgrade_pass,
            )
            .await;
        }

        eprintln!("wait for the upgrade steps to be refreshed ...");
        let latest_sns_version_hash = hash_sns_wasms(&latest_sns_version);
        await_with_timeout(
            &pocket_ic,
            UPGRADE_STEPS_INTERVAL_REFRESH_BACKOFF_SECONDS
                ..EXPECTED_UPGRADE_STEPS_REFRESH_MAX_SECONDS,
            |pocket_ic| async {
                sns::governance::try_get_upgrade_journal(pocket_ic, sns.governance.canister_id)
                    .await
                    .ok()
                    .and_then(|journal| journal.upgrade_steps)
                    .and_then(|upgrade_steps| upgrade_steps.versions.last().cloned())
            },
            &Some(latest_sns_version_hash.clone()),
        )
        .await
        .unwrap();

        eprintln!("advance the target version to the latest version. ...");
        sns::governance::propose_to_advance_sns_target_version(
            &pocket_ic,
            sns.governance.canister_id,
        )
        .await
        .unwrap();

        eprintln!("wait for the upgrade to happen ...");
        await_with_timeout(
            &pocket_ic,
            0..EXPECTED_UPGRADE_DURATION_MAX_SECONDS,
            |pocket_ic| async {
                let journal =
                    sns::governance::try_get_upgrade_journal(pocket_ic, sns.governance.canister_id)
                        .await;
                journal.ok().and_then(|journal| journal.deployed_version)
            },
            &Some(latest_sns_version_hash.clone()),
        )
        .await
        .unwrap();
    }
}
