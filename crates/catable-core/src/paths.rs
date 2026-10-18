use crate::error::CataleError;
use directories::ProjectDirs;
use std::path::PathBuf;

pub fn app_data_dir() -> Result<PathBuf, CataleError> {
    if let Ok(dir) = std::env::var("CATABLE_DATA_DIR") {
        let p = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&p);
        return Ok(p);
    }

    let proj = ProjectDirs::from("com", "fathforce", "catable")
        .ok_or_else(|| CataleError::Io("Cannot locate user home directory".into()))?;
    let dir = proj.data_local_dir().to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| CataleError::Io(e.to_string()))?;
    Ok(dir)
}
