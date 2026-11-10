use crate::{replacer::Replacer, Error, Result};
use std::{
    borrow::Cow,
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

pub(crate) enum PreparedWrite<'a> {
    Unchanged,
    Overwrite(PathBuf, Cow<'a, [u8]>),
    Replace(PathBuf, tempfile::TempPath),
}

impl PreparedWrite<'_> {
    pub(crate) fn commit(self, sync: bool) -> Result<()> {
        match self {
            Self::Unchanged => {}
            Self::Overwrite(path, data) => {
                let mut writer = BufWriter::new(fs::File::create(&path)?);
                writer.write_all(&data)?;
                writer.flush()?;
                if sync {
                    writer.get_ref().sync_all()?;
                }
            }
            Self::Replace(path, temp) => {
                temp.persist(&path)
                    .map_err(|error| Error::TempfilePersist(error.error))?;
                #[cfg(unix)]
                if sync {
                    let parent = path
                        .parent()
                        .ok_or_else(|| Error::InvalidPath(path.clone()))?;
                    fs::File::open(parent)?.sync_all()?;
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn prepare_write<'a>(
    path: &'a Path,
    original: &'a [u8],
    replacer: &'a Replacer,
    sync: bool,
    update_metadata: bool,
) -> Result<PreparedWrite<'a>> {
    let data = replacer.replace(original);

    if data.as_ref() == original && !update_metadata {
        return Ok(PreparedWrite::Unchanged);
    }

    let path = fs::canonicalize(path)?;
    let metadata = fs::metadata(&path)?;
    if metadata.len() == data.len() as u64 {
        return Ok(PreparedWrite::Overwrite(path, data));
    }
    let temp = write_via_tempfile(&path, &metadata, &data, sync)?;
    Ok(PreparedWrite::Replace(path, temp))
}

fn write_via_tempfile(
    path: &Path,
    metadata: &fs::Metadata,
    data: &[u8],
    sync: bool,
) -> Result<tempfile::TempPath> {
    let temp = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| Error::InvalidPath(path.to_path_buf()))?,
    )?;
    let mut file = temp.as_file();

    file.set_permissions(metadata.permissions())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{fchown, MetadataExt};
        fchown(file, Some(metadata.uid()), Some(metadata.gid()))?;
    }
    file.write_all(data)?;
    if sync {
        file.sync_all()?;
    }
    Ok(temp.into_temp_path())
}
