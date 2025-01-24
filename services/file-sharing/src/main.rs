use blake2::Digest;
use byte_unit::Byte;
use clap::{Parser, Subcommand};
use routeweaver_common::ApplicationId;
use std::{path::PathBuf, sync::LazyLock};

mod client;
mod manifest;
mod subcommands;

static APPLICATION_ID: LazyLock<ApplicationId> = LazyLock::new(|| ApplicationId::new("fs"));

#[derive(Subcommand, Debug)]
pub enum ManifestAction {
    Create {
        #[arg(required=true, num_args=1..)]
        paths: Vec<PathBuf>,
    },
    Verify {
        #[arg(required=true, num_args=1..)]
        paths: Vec<PathBuf>,
    },
    Tree,
}

#[derive(Subcommand, Debug)]
pub enum CliActions {
    Serve {
        paths: Vec<PathBuf>,
        #[clap(short, long, default_value = "5 gib")]
        max_cache_size: Byte,
    },
    Download {
        manifest: PathBuf,
        path: PathBuf,
    },
    Manifest {
        manifest_path: PathBuf,
        #[clap(subcommand)]
        action: ManifestAction,
    },
    Mount {
        manifest: PathBuf,
        path: PathBuf,
    },
}

#[derive(Parser, Debug)]
pub struct Cli {
    #[clap(subcommand)]
    action: CliActions,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.action {
        CliActions::Serve {
            paths,
            max_cache_size,
        } => subcommands::serve(paths, max_cache_size.as_u64()).await,
        CliActions::Download { manifest, path } => todo!(),
        CliActions::Manifest {
            manifest_path,
            action,
        } => match action {
            ManifestAction::Create { paths } => todo!(),
            ManifestAction::Verify { paths } => todo!(),
            ManifestAction::Tree => todo!(),
        },
        CliActions::Mount { manifest, path } => todo!(),
    }
}
