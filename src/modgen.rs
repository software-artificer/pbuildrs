use std::{
    collections, ffi, fs,
    io::{self, Write},
    os::unix::ffi::OsStrExt,
    path,
};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to read the directory containing generated Rust source files: {0}")]
    ReadSourceDir(#[from] walkdir::Error),
    #[error("Failed to extract the file name from {0}")]
    FileName(path::PathBuf),
    #[error("Failed to create the module directory `{1}`: {0}")]
    MkModDir(io::Error, path::PathBuf),
    #[error("Failed to create the module file `{1}`: {0}")]
    MkModFile(io::Error, path::PathBuf),
    #[error("Failed to write the module file `{1}`: {0}")]
    WriteModFile(io::Error, path::PathBuf),
    #[error("Failed to read the source file `{1}`: {0}")]
    ReadSourceFile(io::Error, path::PathBuf),
}

struct Tree {
    root: Node,
}

impl Tree {
    fn new() -> Self {
        Self { root: Node::new() }
    }

    fn push(mut self, path: path::PathBuf) -> Result<Self, Error> {
        let file_name = path
            .file_name()
            .ok_or_else(|| Error::FileName(path.to_owned()))?;
        let file_name = path::PathBuf::from(file_name);

        let mut package = file_name.with_extension("");
        let mut parts = vec![];

        while let Some(ext) = package.extension() {
            parts.push(ext.to_owned());

            package.set_extension("");
        }

        parts.push(package.into_os_string());

        self.root = self.root.push(path, parts);

        Ok(self)
    }

    fn compile(self, dst: &path::Path) -> Result<(), Error> {
        self.root.compile(dst.to_owned())
    }
}

#[derive(PartialEq, Debug)]
struct Node {
    path: Option<path::PathBuf>,
    children: collections::HashMap<ffi::OsString, Node>,
}

impl Node {
    fn new() -> Self {
        Node {
            path: None,
            children: Default::default(),
        }
    }

    fn push(mut self, path: path::PathBuf, mut package: Vec<ffi::OsString>) -> Node {
        match package.pop() {
            None => {
                self.path = Some(path);

                self
            }
            Some(part) if part == "_" => self.push(path, package),
            Some(part) => {
                let child = self.children.remove(&part).unwrap_or_else(Node::new);

                self.children.insert(part, child.push(path, package));

                self
            }
        }
    }

    fn compile(self, dst: path::PathBuf) -> Result<(), Error> {
        fs::create_dir_all(&dst).map_err(|err| Error::MkModDir(err, dst.clone()))?;

        let has_children = !self.children.is_empty();

        let mut children = self.children.into_iter().try_fold(
            vec![],
            |mut children, (module, node)| -> Result<_, Error> {
                node.compile(dst.join(&module))?;

                children.push(module);

                Ok(children)
            },
        )?;

        let dst = dst.join("mod.rs");
        let mut mod_file =
            fs::File::create_new(&dst).map_err(|e| Error::MkModFile(e, dst.clone()))?;

        children.sort();
        children
            .into_iter()
            .try_for_each(|module| -> Result<(), Error> {
                mod_file
                    .write(b"pub mod ")
                    .map_err(|e| Error::WriteModFile(e, dst.clone()))?;
                mod_file
                    .write(module.as_bytes())
                    .map_err(|e| Error::WriteModFile(e, dst.clone()))?;
                mod_file
                    .write(b";\n")
                    .map_err(|e| Error::WriteModFile(e, dst.clone()))?;

                Ok(())
            })?;

        if let Some(src) = self.path {
            let contents =
                fs::read_to_string(&src).map_err(|e| Error::ReadSourceFile(e, src.clone()))?;

            if has_children {
                mod_file
                    .write(b"\n")
                    .map_err(|e| Error::WriteModFile(e, dst.clone()))?;
            }

            mod_file
                .write_all(contents.as_bytes())
                .map_err(|e| Error::WriteModFile(e, dst.clone()))?;
        }

        Ok(())
    }
}

#[inline(always)]
fn is_rust_file(e: &walkdir::DirEntry) -> bool {
    e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "rs")
}

pub fn modularize(src: &path::Path, dst: &path::Path) -> Result<(), Error> {
    let files = walkdir::WalkDir::new(src)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    let tree = files
        .into_iter()
        .filter(is_rust_file)
        .try_fold(Tree::new(), |tree, entry| tree.push(entry.into_path()))?;

    tree.compile(dst)
}

#[cfg(test)]
mod tests {
    use std::{collections, ffi, fs, os::unix::fs::PermissionsExt, path};

    #[test]
    fn node_push_no_namespace() {
        let tree = super::Tree::new()
            .push(path::PathBuf::from("/foo/_.rs"))
            .expect("Failed to push a node into a tree");

        assert_eq!(
            tree.root,
            super::Node {
                path: Some(path::PathBuf::from("/foo/_.rs")),
                children: collections::HashMap::new(),
            },
            "The parsed tree has invalid structure",
        );
    }

    #[test]
    fn node_push_valid_namespace() {
        let tree = super::Tree::new()
            .push(path::PathBuf::from("/tmp/foo/crabs.rs"))
            .expect("Failed to push a node into the tree");

        assert_eq!(
            tree.root,
            super::Node {
                path: None,
                children: collections::HashMap::from([(
                    ffi::OsString::from("crabs"),
                    super::Node {
                        children: Default::default(),
                        path: Some(path::PathBuf::from("/tmp/foo/crabs.rs")),
                    }
                )])
            },
            "The parsed tree has invalid structure",
        );
    }

    #[test]
    fn node_push_multiple() {
        let tree = super::Tree::new()
            .push(path::PathBuf::from("/tmp/proto/crabs.disney.ariel.rs"))
            .and_then(|t| t.push(path::PathBuf::from("/tmp/proto/crabs.sponge_bob.rs")))
            .and_then(|t| t.push(path::PathBuf::from("/tmp/proto/crabs.rs")))
            .expect("Failed to push nodes into the tree");

        assert_eq!(
            tree.root,
            super::Node {
                path: None,
                children: collections::HashMap::from([(
                    ffi::OsString::from("crabs"),
                    super::Node {
                        path: Some(path::PathBuf::from("/tmp/proto/crabs.rs")),
                        children: collections::HashMap::from([
                            (
                                ffi::OsString::from("sponge_bob"),
                                super::Node {
                                    path: Some(path::PathBuf::from(
                                        "/tmp/proto/crabs.sponge_bob.rs"
                                    )),
                                    children: collections::HashMap::new(),
                                }
                            ),
                            (
                                ffi::OsString::from("disney"),
                                super::Node {
                                    path: None,
                                    children: collections::HashMap::from([(
                                        ffi::OsString::from("ariel"),
                                        super::Node {
                                            path: Some(path::PathBuf::from(
                                                "/tmp/proto/crabs.disney.ariel.rs"
                                            )),
                                            children: collections::HashMap::new(),
                                        },
                                    )]),
                                }
                            ),
                        ]),
                    }
                )]),
            },
            "The parsed tree has invalid structure",
        );
    }

    #[test]
    fn modularize() {
        let dst =
            tempfile::TempDir::new().expect("Failed to create destination directory for tests");

        let src = tempfile::TempDir::new().expect("Failed to create source directory for tests");

        let root_file = src.path().join("_.rs");
        fs::write(root_file, b"struct Root;\n")
            .expect("Failed to create a root source file for tests");

        let branch_file = src.path().join("a.b.c.rs");
        fs::write(branch_file, b"struct Branch;\n")
            .expect("Failed to create a leaf source file for tests");

        let leaf_file = src.path().join("a.b.c.d.rs");
        fs::write(leaf_file, b"struct Leaf;\n")
            .expect("Failed to create a branch source file for tests");

        let parallel_file = src.path().join("z.rs");
        fs::write(parallel_file, b"struct Parallel;\n")
            .expect("Failed to create a parallel source file for tests");

        super::modularize(src.path(), dst.path()).expect("Failed to modularize the files");

        let output = fs::read_to_string(dst.path().join("a/b/c/d/mod.rs"))
            .expect("Unable to read output file");
        assert_eq!(
            "struct Leaf;\n", output,
            "Invalid contents of the output leaf module `d`",
        );

        let output = fs::read_to_string(dst.path().join("a/b/c/mod.rs"))
            .expect("Unable to read output file");
        assert_eq!(
            "pub mod d;\n\nstruct Branch;\n", output,
            "Invalid contents of the output branch module `c`",
        );

        let output =
            fs::read_to_string(dst.path().join("a/b/mod.rs")).expect("Unable to read output file");
        assert_eq!(
            "pub mod c;\n", output,
            "Invalid contents of the output branch module `b`",
        );

        let output =
            fs::read_to_string(dst.path().join("a/mod.rs")).expect("Unable to read output file");
        assert_eq!(
            "pub mod b;\n", output,
            "Invalid contents of the output branch module `a`",
        );

        let output =
            fs::read_to_string(dst.path().join("mod.rs")).expect("Unable to read output file");
        assert_eq!(
            "pub mod a;\npub mod z;\n\nstruct Root;\n", output,
            "Invalid contents of the output root module",
        );

        let output =
            fs::read_to_string(dst.path().join("z/mod.rs")).expect("Unable to read output file");
        assert_eq!(
            "struct Parallel;\n", output,
            "Invalid contents of the output parallel module",
        );
    }

    #[test]
    fn modularize_walkdir_fails() {
        let dst =
            tempfile::TempDir::new().expect("Failed to create destination directory for tests");

        let src = tempfile::TempDir::new().expect("Failed to create source directory for tests");
        let metadata =
            fs::metadata(&src).expect("Failed to read metadata of the source directory for tests");
        let mut perms = metadata.permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&src, perms)
            .expect("Failed to set permissions on the source directory for tests");

        let err = super::modularize(src.path(), dst.path());
        assert!(
            matches!(err, Err(super::Error::ReadSourceDir { .. })),
            "Expected `Err(Error::ReadSourceDir)`, got: `{:?}`",
            err
        );
    }

    #[test]
    fn modularize_create_dir_fails() {
        let dst =
            tempfile::TempDir::new().expect("Failed to create destination directory for tests");
        let metadata = fs::metadata(&dst)
            .expect("Failed to read metadata of the destination directory for tests");
        let mut perms = metadata.permissions();
        perms.set_readonly(true);
        fs::set_permissions(&dst, perms)
            .expect("Failed to set permissions on the destination directory for tests");

        let src = tempfile::TempDir::new().expect("Failed to create source directory for tests");

        fs::write(src.path().join("ro.rs"), "struct CreateDirFails;\n")
            .expect("Failed to create a test source file");

        let err = super::modularize(src.path(), dst.path());
        assert!(
            matches!(err, Err(super::Error::MkModDir { .. })),
            "Expected `Err(Error::MkModDir)`, got: `{:?}`",
            err
        );
    }

    #[test]
    fn modularize_create_file_fails() {
        let dst =
            tempfile::TempDir::new().expect("Failed to create destination directory for tests");
        let metadata = fs::metadata(&dst)
            .expect("Failed to read metadata of the destination directory for tests");
        let mut perms = metadata.permissions();
        perms.set_readonly(true);
        fs::set_permissions(&dst, perms)
            .expect("Failed to set permissions on the destination directory for tests");

        let src = tempfile::TempDir::new().expect("Failed to create source directory for tests");

        fs::write(src.path().join("_.rs"), "struct Root;\n")
            .expect("Failed to create a test source file");

        let err = super::modularize(src.path(), dst.path());
        assert!(
            matches!(err, Err(super::Error::MkModFile { .. })),
            "Expected `Err(Error::MkModFile)`, got: `{:?}`",
            err
        );
    }

    #[test]
    fn modularize_read_source_fails() {
        let dst =
            tempfile::TempDir::new().expect("Failed to create destination directory for tests");

        let src = tempfile::TempDir::new().expect("Failed to create source directory for tests");

        let src_file = src.path().join("a.b.c.rs");
        fs::write(&src_file, "struct Unreadable;\n").expect("Failed to create a test source file");
        let metadata =
            fs::metadata(&src_file).expect("Failed to read metadata of the test source file");
        let mut perms = metadata.permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&src_file, perms)
            .expect("Failed to set permissions on the test source file");

        let err = super::modularize(src.path(), dst.path());
        assert!(
            matches!(err, Err(super::Error::ReadSourceFile { .. })),
            "Expected `Err(Error::ReadSourceFile)`, got: `{:?}`",
            err
        );
    }
}
