use crate::config::Config;
use crate::ingest::worker::{IngestionWorkerHandle, QueuedIngestionWorker};
use crate::mcp::tools::FoiaSearchServer;
use crate::sources::{
    aaro::AaroAdapter, army::ArmyAdapter, cia::CiaAdapter, dia::DiaAdapter,
    doj_epstein::DojEpsteinAdapter, doj_foia::DojFoiaAdapter, dtic::DticAdapter,
    fbi_vault::FbiVaultAdapter, frus::FrusAdapter, govinfo::GovInfoAdapter, nara::NaraAdapter,
    navy::NavyAdapter, noaa::NoaaAdapter, nsa::NsaAdapter, osd_joint_staff::OsdJointStaffAdapter,
    pursue::PursueAdapter, state::StateAdapter, SourceAdapter,
};
use std::sync::Arc;

pub struct FoiaSearchRuntime {
    server: FoiaSearchServer,
    worker: IngestionWorkerHandle,
}

impl FoiaSearchRuntime {
    pub fn create() -> anyhow::Result<Self> {
        let config = Arc::new(Config::from_env());
        let sources = Arc::new(configured_sources(&config));
        let worker =
            QueuedIngestionWorker::new(config.data_dir.clone(), sources.iter().cloned().collect())
                .with_ocr_policy(config.ocr_fallback_policy)
                .with_ocr_backend(config.ocr_backend.clone())
                .spawn();
        let server = match worker.kick_handle() {
            Some(kick) => FoiaSearchServer::from_parts(config, sources).with_ingestion_worker(kick),
            None => FoiaSearchServer::from_parts(config, sources),
        };

        Ok(Self { server, worker })
    }

    pub fn server(&self) -> FoiaSearchServer {
        self.server.clone()
    }

    pub fn shutdown(self) {
        self.worker.shutdown();
    }
}

fn configured_sources(config: &Config) -> Vec<Arc<dyn SourceAdapter>> {
    vec![
        Arc::new(AaroAdapter::from_env()),
        Arc::new(CiaAdapter::from_env()),
        Arc::new(NaraAdapter::new(
            config.nara_api_base_url.clone(),
            config.nara_api_key.clone(),
        )),
        Arc::new(GovInfoAdapter::default()),
        Arc::new(PursueAdapter::from_env()),
        Arc::new(DojEpsteinAdapter::from_env()),
        Arc::new(DojFoiaAdapter::from_env()),
        Arc::new(FbiVaultAdapter::from_env()),
        Arc::new(FrusAdapter::from_env()),
        Arc::new(DticAdapter::from_env()),
        Arc::new(NavyAdapter::from_env()),
        Arc::new(NoaaAdapter::from_env()),
        Arc::new(NsaAdapter::from_env()),
        Arc::new(StateAdapter::from_env()),
        Arc::new(DiaAdapter::from_env()),
        Arc::new(OsdJointStaffAdapter::from_env()),
        Arc::new(ArmyAdapter::from_env()),
    ]
}
