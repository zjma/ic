#!/bin/bash

# We hold this in memory since it rarely changes (not during a script call)
MAINNET_CANISTER_WASM_HASH_VERSIONS=""
sns_mainnet_canister_wasm_hash_versions() {
    local VERSION_INDEX=${1:--1} # default to last
    MAINNET_CANISTER_WASM_HASH_VERSIONS=${MAINNET_CANISTER_WASM_HASH_VERSIONS:-$(sns_w_list_upgrade_steps ic)}

    echo "$MAINNET_CANISTER_WASM_HASH_VERSIONS" | $IDL2JSON | jq -c ".steps[$VERSION_INDEX] | .pretty_version[0]"
}

sns_mainnet_wasm_hash() {
    local SNS_CANISTER_TYPE=$1
    local VERSION_INDEX=${2:--1} # default to latest
    sns_mainnet_canister_wasm_hash_versions "$VERSION_INDEX" \
        | jq -r ".${SNS_CANISTER_TYPE}_wasm_hash"
}

sns_wasm_hash_to_git_commit() {
    local HASH=$1
    cat "$NNS_TOOLS_DIR/sns_publish_log.txt" \
        | grep "$HASH" \
        | awk '{ print $3 }'
}

sns_mainnet_git_commit_id() {
    local SNS_CANISTER_TYPE=$1
    local VERSION_INDEX=${2:--1} #default to latest
    sns_wasm_hash_to_git_commit "$(sns_mainnet_wasm_hash $SNS_CANISTER_TYPE "$VERSION_INDEX")"
}

create_sns_for_upgrade_test() {
    ensure_variable_set IDL2JSON
    ensure_variable_set LOG_FILE

    local NNS_URL=$1
    local NEURON_ID=$2
    local PEM=$3
    local VERSION_INDEX=$4

    echo "Reset versions to mainnet" | tee -a "${LOG_FILE}"
    reset_sns_w_versions_to_mainnet "$NNS_URL" "$NEURON_ID" "$VERSION_INDEX"
    # propose new SNS
    echo "Proposing new SNS!" | tee -a "${LOG_FILE}"

    if ! propose_new_sns "$NNS_URL" "$NEURON_ID"; then
        print_red "Failed to create a new SNS via 1-proposal initialization"
        exit 1
    fi
    # get the canister ID for the new SNS Governance
    echo "Proposed new SNS" | tee -a $LOG_FILE

    echo "Get the latest SNS canisters and create the sns_canister_ids.json file ..." | tee -a $LOG_FILE
    SNS=$(list_deployed_snses "${NNS_URL}" | $IDL2JSON | jq '.instances[-1]')
    echo "$SNS" | jq '{
            governance_canister_id: .governance_canister_id[0],
            ledger_canister_id: .ledger_canister_id[0],
            root_canister_id: .root_canister_id[0],
            swap_canister_id: .swap_canister_id[0],
            index_canister_id: .index_canister_id[0]
        }' >$PWD/sns_canister_ids.json

    echo "${SNS}" | tee -a $LOG_FILE

    GOV_CANISTER_ID=$(sns_canister_id_for_sns_canister_type governance)
    ROOT_CANISTER_ID=$(sns_canister_id_for_sns_canister_type root)
    SWAP_CANISTER_ID=$(sns_canister_id_for_sns_canister_type swap)
    LEDGER_CANISTER_ID=$(sns_canister_id_for_sns_canister_type ledger)

    echo "Participate in Swap to commit it (this spawns the archive canister) ..." | tee -a $LOG_FILE
    sns_quill_participate_in_sale "${NNS_URL}" "${PEM}" "${ROOT_CANISTER_ID}" 300000

    echo "Wait for finalization to complete ..." | tee -a "${LOG_FILE}"
    if ! wait_for_sns_governance_to_be_in_normal_mode "${NNS_URL}" "${GOV_CANISTER_ID}"; then
        print_red "Swap finalization failed, cannot continue with upgrade testing"
        exit 1
    fi

    echo "Add the archive canister to sns_canister_ids.json for use during upgrade testing ..." | tee -a $LOG_FILE
    ARCHIVE_CANISTER_ID=$(sns_get_archive "${NNS_URL}" "${LEDGER_CANISTER_ID}")
    add_archive_to_sns_canister_ids "$PWD/sns_canister_ids.json" "${ARCHIVE_CANISTER_ID}"

    echo "Assert that all canisters have the mainnet hashes so our test is legitimate ..." | tee -a $LOG_FILE
    canister_has_hash_installed $NNS_URL \
        $(sns_canister_id_for_sns_canister_type governance) $(sns_mainnet_wasm_hash governance ${VERSION_INDEX})
    canister_has_hash_installed $NNS_URL \
        $(sns_canister_id_for_sns_canister_type root) $(sns_mainnet_wasm_hash root ${VERSION_INDEX})
    canister_has_hash_installed $NNS_URL \
        $(sns_canister_id_for_sns_canister_type ledger) $(sns_mainnet_wasm_hash ledger ${VERSION_INDEX})
    canister_has_hash_installed $NNS_URL \
        $(sns_canister_id_for_sns_canister_type index) $(sns_mainnet_wasm_hash index ${VERSION_INDEX})
    canister_has_hash_installed $NNS_URL \
        $(sns_canister_id_for_sns_canister_type swap) $(sns_mainnet_wasm_hash swap ${VERSION_INDEX})
    canister_has_hash_installed $NNS_URL \
        $(sns_canister_id_for_sns_canister_type archive) $(sns_mainnet_wasm_hash archive ${VERSION_INDEX})

}

reset_sns_w_versions_to_mainnet() {
    ensure_variable_set IDL2JSON

    local NNS_URL=$1
    local NEURON_ID=$2
    local VERSION_INDEX=${3:--1}

    upload_canister_git_version_to_sns_wasm \
        "$NNS_URL" "$NEURON_ID" \
        "$PEM" \
        root $(sns_mainnet_git_commit_id root "${VERSION_INDEX}")

    upload_canister_git_version_to_sns_wasm \
        "$NNS_URL" "$NEURON_ID" \
        "$PEM" \
        governance $(sns_mainnet_git_commit_id governance "${VERSION_INDEX}")

    upload_canister_git_version_to_sns_wasm \
        "$NNS_URL" "$NEURON_ID" \
        "$PEM" \
        ledger $(sns_mainnet_git_commit_id ledger "${VERSION_INDEX}")

    upload_canister_git_version_to_sns_wasm \
        "$NNS_URL" "$NEURON_ID" \
        "$PEM" \
        archive $(sns_mainnet_git_commit_id archive "${VERSION_INDEX}")

    upload_canister_git_version_to_sns_wasm \
        "$NNS_URL" "$NEURON_ID" \
        "$PEM" \
        swap $(sns_mainnet_git_commit_id swap "${VERSION_INDEX}")

    upload_canister_git_version_to_sns_wasm \
        "$NNS_URL" "$NEURON_ID" \
        "$PEM" \
        index $(sns_mainnet_git_commit_id index "${VERSION_INDEX}")
}

upload_canister_git_version_to_sns_wasm() {
    ensure_variable_set IC_ADMIN

    local NNS_URL=$1 # with protocol and port (http://...:8080)
    local NEURON_ID=$2
    local PEM=$3
    local CANISTER_TYPE=$4
    local VERSION=$5

    WASM_GZ=$(download_sns_canister_wasm_gz_for_type "$CANISTER_TYPE" "$VERSION")

    upload_wasm_to_sns_wasm "$NNS_URL" "$NEURON_ID" \
        "$PEM" "$CANISTER_TYPE" "$WASM_GZ"
}

upload_wasm_to_sns_wasm() {
    ensure_variable_set IC_ADMIN

    local NNS_URL=$1 # with protocol and port (http://...:8080)
    local NEURON_ID=$2
    local PEM=$3
    local CANISTER_TYPE=$4
    local WASM=$5

    WASM_SHA=$(sha_256 "$WASM")

    SUMMARY_FILE=$(mktemp)
    echo "Proposal to add a WASM" >$SUMMARY_FILE

    # We ignore most of the output here because it overwhelms the terminal
    $IC_ADMIN -s "$PEM" --nns-url "$NNS_URL" \
        propose-to-add-wasm-to-sns-wasm \
        --wasm-module-path "$WASM" \
        --wasm-module-sha256 "$WASM_SHA" \
        --canister-type "$CANISTER_TYPE" \
        --summary-file "$SUMMARY_FILE" \
        --proposer "$NEURON_ID" \
        | grep proposal
}

insert_sns_wasm_upgrade_paths_for_all_snses() {
    ensure_variable_set IC_ADMIN

    local NNS_URL=$1
    local NEURON_ID=$2
    local PEM=$3
    shift 3
    local VERSIONS_ARR=()
    for VERSION in $(generate_versions_from_initial_and_diffs "${@}"); do
        VERSIONS_ARR+=("$VERSION")
    done

    SUMMARY_FILE=$(mktemp)
    echo "Proposal to add upgrade paths" >$SUMMARY_FILE

    $IC_ADMIN -s "$PEM" --nns-url "$NNS_URL" \
        propose-to-insert-sns-wasm-upgrade-path-entries \
        --force-upgrade-main-upgrade-path true \
        --summary-file=$SUMMARY_FILE \
        --proposer=$NEURON_ID \
        "${VERSIONS_ARR[@]}"
}

propose_new_sns() {
    ensure_variable_set SNS_CLI

    local NNS_URL=$1
    local NEURON_ID=$2
    local CONFIG_FILE=${3:-}

    if [ -z "$CONFIG_FILE" ]; then
        CONFIG_FILE=$NNS_TOOLS_DIR/sns_default_test_init_params_v2.yml
    fi

    set +e

    OUT=$(HOME=${DFX_HOME:-HOME} $SNS_CLI propose --network "${NNS_URL}" \
        --neuron-id "${NEURON_ID}" \
        "${CONFIG_FILE}")
    set -e

    PROPOSAL_ID=$(echo "$OUT" | grep -o 'Proposal ID: [0-9]*' | cut -d' ' -f3)
    if ! wait_for_proposal_to_execute "$NNS_URL" "$PROPOSAL_ID"; then
        print_red >&2 "Failed to execute proposal $PROPOSAL_ID to create a new SNS"
        return 1
    fi

    return 0
}

set_sns_wasms_allowed_subnets() {
    ensure_variable_set IC_ADMIN

    local NNS_URL=$1 # with protocol and port (http://...:8080)
    local NEURON_ID=$2
    local PEM=$3
    local SUBNET_TO_ADD=$4

    #  Remove all from current list
    #  and add new one

    CURRENT_SUBNETS=$(__dfx -q canister --network "$NNS_URL" call qaa6y-5yaaa-aaaaa-aaafa-cai get_sns_subnet_ids '(record {})' \
        | grep principal \
        | sed 's/.*"\(.*\)";/\1/')

    cmd=($IC_ADMIN --nns-url $NNS_URL -s $PEM propose-to-update-sns-subnet-ids-in-sns-wasm --summary "Updating SNS subnet ids in SNS-WASM")

    for current_subnet in $CURRENT_SUBNETS; do
        cmd+=(--sns-subnet-ids-to-remove $current_subnet)
    done

    cmd+=(--sns-subnet-ids-to-add $SUBNET_TO_ADD)

    cmd+=(--proposer $NEURON_ID)

    "${cmd[@]}"
}

wait_for_sns_canister_has_version() {
    local NETWORK=$1
    local CANISTER_ID=$2
    local SNS_CANISTER_TYPE=$3
    local VERSION=$4

    WASM=$(download_sns_canister_wasm_gz_for_type $SNS_CANISTER_TYPE $VERSION)
    wait_for_canister_has_file_contents "$NETWORK" "$CANISTER_ID" "$WASM"
}

wait_for_sns_canister_has_hash() {
    local NETWORK=$1
    local CANISTER_ID=$2
    local HASH=$3

    for i in {1..20}; do
        echo "Testing if upgrade was successful..."
        if canister_has_hash_installed "$NETWORK" "$CANISTER_ID" "$HASH"; then
            print_green "Canister $CANISTER_ID successfully upgraded."
            return 0
        fi
        sleep 10
    done

    print_red "Canister $CANISTER_ID upgrade failed"
    return 1
}

wait_for_canister_has_file_contents() {
    local NETWORK=$1
    local CANISTER_ID=$2
    local WASM=$3

    for i in {1..20}; do
        echo "Testing if upgrade was successful..."
        if canister_has_file_contents_installed "$NETWORK" "$CANISTER_ID" "$WASM"; then
            print_green "Canister $CANISTER_ID successfully upgraded."
            return 0
        fi
        sleep 10
    done

    print_red "Canister $CANISTER_ID upgrade failed"
    return 1
}

sns_canister_id_for_sns_canister_type() {
    local SNS_CANISTER_TYPE=$1
    cat $PWD/sns_canister_ids.json | jq -r ".${SNS_CANISTER_TYPE}_canister_id"
}

upgrade_swap() {
    NNS_URL=$1
    NEURON_ID=$2
    PEM=$3
    CANISTER_ID=$4
    VERSION_OR_WASM=$5

    WASM_FILE=$([ -f "$VERSION_OR_WASM" ] && echo "$VERSION_OR_WASM" || download_sns_canister_wasm_gz_for_type swap "$VERSION")

    propose_upgrade_canister_wasm_file_pem "$NNS_URL" "$NEURON_ID" "$PEM" "$CANISTER_ID" "$WASM_FILE"
}

upgrade_sns() {
    NNS_URL=$1
    NEURON_ID=$2
    PEM=$3
    CANISTER_NAME=$4
    VERSION_OR_WASM=$5
    LOG_FILE=$6
    SWAP_CANISTER_ID=$7
    GOV_CANISTER_ID=$8

    # For swap testing, we want to do the NNS upgrade
    if [[ $CANISTER_NAME = "swap" ]]; then
        echo "Submitting upgrade proposal to NNS Governance for Swap" | tee -a "$LOG_FILE"
        upgrade_swap "$NNS_URL" "$NEURON_ID" "$PEM" "$SWAP_CANISTER_ID" "$VERSION_OR_WASM"
    fi

    # SNS upgrade proposal - needed even if swap was upgraded
    echo "Submitting upgrade proposal to $GOV_CANISTER_ID" | tee -a "$LOG_FILE"
    sns_upgrade_to_next_version "$NNS_URL" "$PEM" "$GOV_CANISTER_ID" 0
}

upgrade_nns_governance_to_test_version() {
    NNS_URL=$1
    NEURON_ID=$2
    PEM=$3

    GOVERNANCE_CANISTER_ID=$(nns_canister_id governance)
    GIT_COMMIT=$(canister_git_version "${NNS_URL}" "${GOVERNANCE_CANISTER_ID}")
    DOWNLOAD_NAME="governance-canister_test"
    WASM_GZ_FILE=$(_download_canister_gz "${DOWNLOAD_NAME}" "${GIT_COMMIT}")
    WASM_SHA=$(sha_256 "${WASM_GZ_FILE}")

    if nns_canister_has_file_contents_installed "${NNS_URL}" "governance" "${WASM_GZ_FILE}"; then
        print_green "Governance already on the correct version."
        return 0
    fi

    propose_upgrade_nns_canister_wasm_file_pem "${NNS_URL}" "${NEURON_ID}" "${PEM}" "governance" "${WASM_GZ_FILE}"

    if ! wait_for_nns_canister_has_file_contents "${NNS_URL}" "governance" "${WASM_GZ_FILE}"; then
        print_red "Could not upgrade NNS Governance to its test version at version ${GIT_COMMIT}"
        exit 1
    fi

    print_green "Upgraded NNS Governance to its test build for Git Commit ${GIT_COMMIT}. Its hash is ${WASM_SHA}"
}
