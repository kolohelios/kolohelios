//! `shaka object-store` — manage the shared `kolohelios` Linode Object
//! Storage bucket and the namespace registry that protects it.
//!
//! The bucket holds heterogeneous content (Terraform state, future caches,
//! assets) under top-level prefixes called *namespaces*. Namespaces are
//! declared in each consuming project's `project.cue` so demand stays
//! adjacent to the project that owns it; shaka aggregates the declarations
//! into a registry and audits the live bucket against it.

pub mod client;
pub mod init;
pub mod ns;
pub mod registry;
pub mod status;
pub mod tfstate;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum ObjectStoreCommand {
    /// One-time bootstrap: create the kolohelios bucket and bucket-scoped
    /// access keys. Idempotent — reports and exits 0 if the bucket exists.
    Init {
        /// Linode Object Storage cluster (region) to create the bucket in
        #[arg(long, default_value = "us-sea-1")]
        cluster: String,
        /// Bucket name (rarely overridden — keep it `kolohelios` for the
        /// shared monorepo bucket)
        #[arg(long, default_value = "kolohelios")]
        bucket: String,
    },
    /// Show bucket health and namespace status in one shot
    Status {
        /// Bucket name
        #[arg(long, default_value = "kolohelios")]
        bucket: String,
    },
    /// Namespace registry operations
    Ns {
        #[command(subcommand)]
        command: NsCommand,
    },
    /// Terraform remote-state helpers built on top of the registry
    Tfstate {
        #[command(subcommand)]
        command: TfstateCommand,
    },
}

#[derive(Subcommand)]
pub enum NsCommand {
    /// List every namespace declared across the monorepo's project.cue files
    List,
    /// Audit namespace declarations for collisions and (when creds are
    /// present) drift against the live bucket
    Audit {
        /// Bucket name
        #[arg(long, default_value = "kolohelios")]
        bucket: String,
    },
}

#[derive(Subcommand)]
pub enum TfstateCommand {
    /// Write `backend.tf` for a Terraform module using its registered
    /// tfstate namespace from the module's project.cue
    Emit {
        /// Path to the Terraform module directory (the project that
        /// declares the tfstate namespace must be its enclosing project)
        #[arg(long)]
        module: String,
        /// Refuse to overwrite an existing backend.tf without --force
        #[arg(long)]
        force: bool,
    },
    /// Wrap `tofu init -migrate-state` for a module that just adopted a
    /// remote backend. Sanity-checks AWS_* creds in the environment.
    Migrate {
        /// Path to the Terraform module directory
        #[arg(long)]
        module: String,
    },
}

pub fn run(cmd: ObjectStoreCommand) {
    match cmd {
        ObjectStoreCommand::Init { cluster, bucket } => init::run(&cluster, &bucket),
        ObjectStoreCommand::Status { bucket } => status::run(&bucket),
        ObjectStoreCommand::Ns { command } => match command {
            NsCommand::List => ns::list(),
            NsCommand::Audit { bucket } => ns::audit(&bucket),
        },
        ObjectStoreCommand::Tfstate { command } => match command {
            TfstateCommand::Emit { module, force } => tfstate::emit(&module, force),
            TfstateCommand::Migrate { module } => tfstate::migrate(&module),
        },
    }
}
