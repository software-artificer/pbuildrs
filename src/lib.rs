pub mod cli;
pub mod modgen;
mod patcher;

use rayon::prelude::*;
use std::{fs, io, path};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to read the Protobuf files from `{1}`: {0}")]
    WalkDir(walkdir::Error, path::PathBuf),
    #[error("Failed to resolve the protobuf path `{1}`: {0}")]
    PathResolve(path::StripPrefixError, path::PathBuf),
    #[error("Failed to open the source file `{1}`: {0}")]
    OpenSourceFile(io::Error, path::PathBuf),
    #[error("Failed to create the patched file `{1}`: {0}")]
    OpenTempFile(io::Error, path::PathBuf),
    #[error("Failed to create the destination subdirectory `{1}` for patched files: {0}")]
    CreatePatchedSubdir(io::Error, path::PathBuf),
    #[error("Failed to process the `{1}` protobuf file: {0}")]
    PatchEdition(patcher::Error, path::PathBuf),
}

pub fn patch_protos(
    src_dir: &path::Path,
    dst_dir: &path::Path,
) -> Result<Vec<path::PathBuf>, Error> {
    let files = walkdir::WalkDir::new(src_dir)
        .contents_first(false)
        .into_iter()
        .try_fold(vec![], |mut files, entry| -> Result<_, Error> {
            let path = entry
                .map_err(|e| Error::WalkDir(e, src_dir.to_path_buf()))?
                .path()
                .to_path_buf();

            if path.is_dir() {
                let dst_path = dst_dir.join(
                    path.strip_prefix(src_dir)
                        .map_err(|e| Error::PathResolve(e, src_dir.to_path_buf()))?,
                );

                println!("Creating a subdirectory: {}", dst_path.display());

                fs::create_dir_all(&dst_path)
                    .map_err(|e| Error::CreatePatchedSubdir(e, dst_path))?;
            } else {
                files.push(path);
            }

            Ok(files)
        })?;

    files
        .par_iter()
        .filter(|file| file.extension().is_some_and(|ext| ext == "proto"))
        .map(|proto| {
            let path = proto
                .strip_prefix(src_dir)
                .map_err(|e| Error::PathResolve(e, src_dir.to_path_buf()))?;

            println!("Processing: {}", path.display());

            let src = fs::File::open(proto).map_err(|e| Error::OpenSourceFile(e, proto.clone()))?;

            let output = dst_dir.join(path);
            let dst = fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .create_new(true)
                .open(&output)
                .map_err(|e| Error::OpenTempFile(e, output.clone()))?;

            patcher::patch_edition(io::BufReader::new(src), dst)
                .map_err(|e| Error::PatchEdition(e, proto.to_path_buf()))?;

            Ok(output)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use tempfile::tempdir;

    #[test]
    fn patch_proto_fails_if_it_can_not_read_a_source_directory() {
        let src_dir = tempdir().expect("Failed to create a test source directory");

        let metadata =
            fs::metadata(src_dir.path()).expect("Failed to read test source directory metadata");
        let mut perms = metadata.permissions();

        perms.set_mode(0o000);

        fs::set_permissions(src_dir.path(), perms)
            .expect("Failed to update test source directory permissions");
        let dst_dir = tempdir().expect("Failed to create a test destination directory");

        let err = super::patch_protos(src_dir.path(), dst_dir.path())
            .expect_err("Patcher didn't fail given unreadable directory");

        assert!(
            matches!(err, super::Error::WalkDir { .. }),
            "Expected `patch_protos()` to fail with `Error::WalkDir` variant"
        );

        let err_msg = err.to_string();
        assert!(
            err_msg.starts_with("Failed to read the Protobuf files from `"),
            "Expected `patch_protos()` to fail with with:\n> Failed to read the Protobuf files from `\nmessage, got:\n> {err_msg}",
        );
    }

    #[test]
    fn patch_proto_fails_if_it_can_not_create_a_destination_subdirectory() {
        let src_dir = tempdir().expect("Failed to create a test source directory");

        let bad_dir = src_dir.path().join("bad_subdir");
        fs::create_dir_all(bad_dir).expect("Failed to create a test source subdirectory");

        let dst_dir = tempdir().expect("Failed to create a test destination directory");

        let metadata = fs::metadata(dst_dir.path())
            .expect("Failed to read test destination directory metadata");
        let mut perms = metadata.permissions();

        perms.set_readonly(true);

        fs::set_permissions(dst_dir.path(), perms)
            .expect("Failed to update test destination directory permissions");

        let err = super::patch_protos(src_dir.path(), dst_dir.path())
            .expect_err("Patcher didn't fail given unreadable directory");

        assert!(matches!(err, super::Error::CreatePatchedSubdir { .. }));

        let err_msg = err.to_string();
        assert!(
            err_msg.starts_with("Failed to create the destination subdirectory "),
            "Expected `patched_protos()` to fail with:\n> Failed to create the destination subdirectory\n message, got:\n> {err_msg}"
        );
    }

    #[test]
    fn patch_proto_fails_if_it_can_not_read_a_source_file() {
        let src_dir = tempdir().expect("Failed to create a test source directory");

        let bad_file = src_dir.path().join("bad.proto");
        fs::File::create_new(&bad_file).expect("Failed to create a test protobuf file");

        let metadata = fs::metadata(&bad_file).expect("Failed to read a test proto file metadata");
        let mut perms = metadata.permissions();

        perms.set_mode(0o000);

        fs::set_permissions(bad_file, perms)
            .expect("Failed to set permissions on the test proto file");

        let dst_dir = tempdir().expect("Failed to create a test destination directory");

        let err = super::patch_protos(src_dir.path(), dst_dir.path())
            .expect_err("Patcher didn't fail given unreadable proto file");

        assert!(matches!(err, super::Error::OpenSourceFile { .. }));

        let err_msg = err.to_string();
        assert!(
            err_msg.starts_with("Failed to open the source file"),
            "Expected `patched_protos()` to fail with:\n> Failed to open the source file\n message, got:\n> {err_msg}"
        );
    }

    #[test]
    fn patch_proto_fails_if_it_can_not_create_a_patched_file() {
        let src_dir = tempdir().expect("Failed to create a test source directory");
        let src_file_path = src_dir.path().join("test.proto");

        fs::write(
            src_file_path,
            r#"syntax = "proto3";

package test;

message Foo {
}
"#,
        )
        .expect("Failed to create a test protobuf file");

        let dst_dir = tempdir().expect("Failed to create a test destination directory");

        let metadata = fs::metadata(dst_dir.path())
            .expect("Failed to read a test destination directory metadata");
        let mut perms = metadata.permissions();

        perms.set_mode(0o000);

        fs::set_permissions(dst_dir.path(), perms)
            .expect("Failed to set permissions on the test destination directory");

        let err = super::patch_protos(src_dir.path(), dst_dir.path())
            .expect_err("Patcher didn't fail given unreadable proto file");

        assert!(matches!(err, super::Error::OpenTempFile { .. }));

        let err_msg = err.to_string();
        assert!(
            err_msg.starts_with("Failed to create the patched file"),
            "Expected `patched_protos()` to fail with:\n> Failed to create the patched file\n message, got:\n> {err_msg}"
        );
    }

    #[test]
    fn patch_proto_successfully_handles_file_generation() {
        let src_dir = tempdir().expect("Failed to create a test source directory");
        let src_file_path = src_dir.path().join("test.proto");

        fs::write(
            src_file_path,
            r#"edition = "2023";

package test;

message Foo {
}
"#,
        )
        .expect("Failed to create a test protobuf file");

        let ignore_file_path = src_dir.path().join("README.md");
        fs::write(
            ignore_file_path,
            "This file should be ignored by the patcher",
        )
        .expect("Failed to create an ignorable source file");

        let dst_dir = tempdir().expect("Failed to create a test destination directory");

        let result = super::patch_protos(src_dir.path(), dst_dir.path())
            .expect("Patcher failed to process proto files");

        assert_eq!(
            result.len(),
            1,
            "Expected result to contain a single patched proto file path"
        );

        let patched =
            fs::read_to_string(&result[0]).expect("Failed to read the patched proto filed");

        assert_eq!(
            patched,
            r#"syntax = "proto3";

package test;

message Foo {
}
"#,
            "The patched file content is invalid"
        );
    }
}
