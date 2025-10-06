use std::{fs, io, path};

use crate::modgen;

/// Compile protobuf files into properly structured Rust code with modules using the Prost compiler.
#[derive(clap::Parser)]
#[command(version, about)]
pub struct Args {
    /// Whether to generate the gRPC client code
    #[arg(long, default_value_t = false)]
    build_client: bool,
    /// Whether to generate the gRPC server stubs
    #[arg(long, default_value_t = false)]
    build_server: bool,
    /// Specify whether to build the well-known types
    #[arg(long, default_value_t = false)]
    with_well_known_types: bool,
    /// Add a directory to the Protobuf import path (can be specified multiple times)
    #[arg(long, short = 'I')]
    include_path: Vec<path::PathBuf>,
    /// Specify the output path for the compiled files
    #[arg(long, default_value = "out")]
    output: path::PathBuf,
    /// Specify a path where to create a temporary working directory
    #[arg(long)]
    temp_dir: Option<path::PathBuf>,
    /// Specify the source path of the protobuf files to compile
    #[arg()]
    source: path::PathBuf,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to create a temporary directory: {0}")]
    MkTempDir(io::Error),
    #[error("Failed to remove previous output directory: {0}")]
    RemoveOutDir(io::Error),
    #[error("Failed to create an output directory: {0}")]
    CreateOutDir(io::Error),
    #[error("Failed to compile the proto file: {0}")]
    CompileProto(io::Error),
    #[error("Failed to patch protobuf files: {0}")]
    PatchEdition(#[from] crate::Error),
    #[error("Failed to create a temporary directory for generate source code `{1}`: {0}")]
    MkTempCompileDir(io::Error, path::PathBuf),
    #[error("")]
    Modularize(#[from] modgen::Error),
}

pub fn create_temp_working_dir(
    path: &Option<path::PathBuf>,
) -> Result<tempfile::TempDir, io::Error> {
    let mut tempdir = tempfile::Builder::new();
    let tempdir = tempdir.prefix("pbuildrs-");

    if let Some(path) = path {
        tempdir.tempdir_in(path)
    } else {
        tempdir.tempdir()
    }
}

pub fn run(args: Args) -> Result<(), Error> {
    if args.output.exists() {
        println!("Found previous output directory, cleaning up");
        fs::remove_dir_all(&args.output).map_err(Error::RemoveOutDir)?;
        println!("Previous output directory was removed");
    }

    fs::create_dir_all(&args.output).map_err(Error::CreateOutDir)?;
    println!("Created an output directory: {}", args.output.display());

    let tempdir = create_temp_working_dir(&args.temp_dir).map_err(Error::MkTempDir)?;

    println!(
        "Created a temporary working directory: {}",
        tempdir.path().display(),
    );

    let patched_dir = tempdir.path().join("protos");
    let patched_files = crate::patch_protos(&args.source, &patched_dir)?;

    let compiled_files_dir = tempdir.path().join("code");
    fs::create_dir_all(&compiled_files_dir)
        .map_err(|e| Error::MkTempCompileDir(e, compiled_files_dir.clone()))?;
    println!(
        "Created temporary directory for generated source code: {}",
        compiled_files_dir.display()
    );

    let mut includes = args.include_path;
    includes.push(patched_dir);

    tonic_prost_build::configure()
        .build_client(args.build_client)
        .build_server(args.build_server)
        .build_transport(args.build_client || args.build_server)
        .compile_well_known_types(args.with_well_known_types)
        .out_dir(&compiled_files_dir)
        .compile_protos(&patched_files, &includes)
        .map_err(Error::CompileProto)?;

    modgen::modularize(&compiled_files_dir, &args.output)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path};

    #[test]
    fn run_end_to_end_test() {
        let dst = tempfile::TempDir::new().expect("Failed to create test destination directory");

        let src = path::PathBuf::from("./proto");

        let args = super::Args {
            build_client: true,
            build_server: true,
            with_well_known_types: true,
            include_path: vec![],
            output: dst.path().to_owned(),
            source: src,
            temp_dir: None,
        };

        super::run(args).expect("Failed to run the application");

        let result = fs::read_to_string(dst.path().join("crabs/sponge_bob/mod.rs"))
            .expect("Failed to read the generated file");
        assert!(
            result.contains("struct MrKrabs"),
            "Expected the sponge_bob module to contain `MrKrabs` struct"
        );
        assert!(
            result.contains("struct BetsyKrabs"),
            "Expected the sponge_bob module to contain `BetsyKrabs` struct",
        );

        let result = fs::read_to_string(dst.path().join("crabs/disney/ariel/mod.rs"))
            .expect("Failed to read the generated file");
        assert!(
            result.contains("struct Sebastian"),
            "Expected the ariel module to contain `Sebastian` struct",
        );

        let result = fs::read_to_string(dst.path().join("crabs/mod.rs"))
            .expect("Failed to read the generated file");
        let expected_items = [
            "struct Ferris",
            "enum FerrisType",
            "struct GetFerrisReqProto",
            "struct GetMrKrabsReqProto",
            "struct GetSebastianReqProto",
            "struct GetBetsyKrabsReqProto",
            "struct CrabServiceClient",
            "async fn get_betsy_krabs(",
            "async fn get_sebastian(",
            "async fn get_mr_krabs(",
            "async fn get_ferris(",
            "trait CrabService",
            "struct CrabServiceServer",
            "pub mod disney;",
            "pub mod sponge_bob;",
        ];
        expected_items.into_iter().for_each(|pat| {
            assert!(
                result.contains(pat),
                "Expected generated crabs module to contain `{}` pattern, got:\n{}",
                pat,
                result
            );
        });

        let result = fs::read_to_string(dst.path().join("crabs/disney/mod.rs"))
            .expect("Failed to read the generated file");
        assert_eq!(
            result, "pub mod ariel;\n",
            "Invalid generated module: disney"
        );
    }
}
