pub mod archive;
pub mod governance;
pub mod index;
pub mod ledger;
pub mod root;
pub mod swap;

use anyhow::Result;
use ic_nns_constants::SNS_WASM_CANISTER_ID;
use ic_sns_wasm::pb::v1::{ListUpgradeStepsRequest, ListUpgradeStepsResponse, SnsVersion};
use serde::{Deserialize, Serialize};

use crate::CallCanisters;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Sns {
    pub ledger: ledger::LedgerCanister,
    pub governance: governance::GovernanceCanister,
    pub index: index::IndexCanister,
    pub swap: swap::SwapCanister,
    pub root: root::RootCanister,
    pub archive: Vec<archive::ArchiveCanister>,
}

impl Sns {
    pub async fn remaining_upgrade_steps<C: CallCanisters>(
        &self,
        agent: &C,
    ) -> Result<ListUpgradeStepsResponse, C::Error> {
        let version = self.governance.version(agent).await?;
        let list_upgrade_steps_request = ListUpgradeStepsRequest {
            limit: 0,
            sns_governance_canister_id: Some(self.governance.canister_id),
            starting_at: version.deployed_version.map(SnsVersion::from),
        };
        agent
            .call(SNS_WASM_CANISTER_ID, list_upgrade_steps_request)
            .await
    }
}

impl TryFrom<ic_sns_wasm::pb::v1::DeployedSns> for Sns {
    type Error = String;

    fn try_from(deployed_sns: ic_sns_wasm::pb::v1::DeployedSns) -> Result<Self, Self::Error> {
        Ok(Self {
            ledger: ledger::LedgerCanister {
                canister_id: deployed_sns
                    .ledger_canister_id
                    .ok_or("ledger_canister_id not found")?,
            },
            governance: governance::GovernanceCanister {
                canister_id: deployed_sns
                    .governance_canister_id
                    .ok_or("ledger_canister_id not found")?,
            },
            index: index::IndexCanister {
                canister_id: deployed_sns
                    .index_canister_id
                    .ok_or("ledger_canister_id not found")?,
            },
            swap: swap::SwapCanister {
                canister_id: deployed_sns
                    .swap_canister_id
                    .ok_or("ledger_canister_id not found")?,
            },
            root: root::RootCanister {
                canister_id: deployed_sns
                    .root_canister_id
                    .ok_or("ledger_canister_id not found")?,
            },
            archive: Vec::new(),
        })
    }
}
