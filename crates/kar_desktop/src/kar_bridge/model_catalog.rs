//! A picker reads the selected project's models independently of the all-agent status scan.
use super::*;
use kar::forms::{ListingReason, ModelListing};

impl WorkerState {
    pub(super) fn request_model_catalog(&mut self, runtime: String, workspace: PathBuf) {
        if self
            .catalog_requests
            .get(&runtime)
            .is_some_and(|(path, _)| path == &workspace)
        {
            return;
        }
        let request = Uuid::new_v4();
        self.catalog_requests
            .insert(runtime.clone(), (workspace.clone(), request));
        self.emit(KarBridgeEvent::ModelCatalogUpdated {
            runtime: runtime.clone(),
            workspace: workspace.clone(),
            status: ModelCatalogState::Loading,
            models: Vec::new(),
        });
        let engine = self.project_engine(&workspace);
        let commands = self.cmd_tx.clone();
        tokio::spawn(async move {
            let result = async {
                if crate::aster::is_aster(&runtime) {
                    return Ok((
                        ModelCatalogState::Available,
                        crate::aster::read_models(&workspace).await?,
                    ));
                }
                let engine = engine?;
                let path = format!("/projects/{}/models/{}", engine.project_id(), runtime);
                let response = engine.request(axum::http::Method::GET, &path, None).await?;
                let listing: ModelListing = serde_json::from_value(response)?;
                let status = match &listing {
                    ModelListing::Unavailable {
                        reason: ListingReason::Unsupported,
                    } => ModelCatalogState::Unsupported,
                    ModelListing::Unavailable {
                        reason: ListingReason::Empty,
                    } => ModelCatalogState::Available,
                    ModelListing::Unavailable { .. } => ModelCatalogState::Failed,
                    ModelListing::Live { .. } | ModelListing::Fallback { .. } => {
                        ModelCatalogState::Available
                    }
                };
                let models = listing
                    .models()
                    .iter()
                    .filter_map(ProviderModelOption::from_kar_entry)
                    .collect();
                Ok::<_, anyhow::Error>((status, models))
            }
            .await;
            let (status, models) = result.unwrap_or_else(|error| {
                log::debug!("Model catalogue request for {runtime} failed: {error}");
                (ModelCatalogState::Failed, Vec::new())
            });
            let _ = commands
                .send(KarCommand::ModelCatalogRead {
                    runtime,
                    workspace,
                    request,
                    status,
                    models,
                })
                .await;
        });
    }
}
