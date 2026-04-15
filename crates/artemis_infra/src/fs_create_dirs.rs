use std::path::Path;

use artemis_app::FileDirectoryInfra;

#[derive(Default)]
pub struct ForgeCreateDirsService;

#[async_trait::async_trait]
impl FileDirectoryInfra for ForgeCreateDirsService {
    async fn create_dirs(&self, path: &Path) -> anyhow::Result<()> {
        Ok(artemis_fs::ForgeFS::create_dir_all(path).await?)
    }
}
