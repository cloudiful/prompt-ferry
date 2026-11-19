use clap::{ArgGroup, Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "prompt-ferry",
    version,
    about = "Unified OpenAI-compatible node for upstream ferrying",
    long_about = "Run with no subcommand to start the node: one process serves the public AI \
                  surface and the admin UI/API from a single listener, and it executes requests \
                  locally or forwards them to its database node.\n\nThe `openapi`, `cert`, and \
                  `maintenance` subcommands are operational utilities and do not start the node.",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// Options of the default node startup.
    #[command(flatten)]
    pub node: NodeArgs,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(subcommand)]
    Openapi(OpenapiCommand),
    #[command(subcommand)]
    Cert(CertCommand),
    #[command(subcommand)]
    Maintenance(MaintenanceCommand),
}

/// Options of the default node startup.
///
/// The flag is an acknowledgement, never a secret: the master secret, the
/// database URL, and other settings come from the node configuration or the
/// environment.
#[derive(Debug, Args, Clone, Default)]
pub struct NodeArgs {
    /// Acknowledge that pre-node processes are stopped so the gate may cut
    /// over a legacy schema. The gate only asks for it when it detects one.
    #[arg(long)]
    pub ack_stop_old_nodes: bool,
}

#[derive(Debug, Subcommand)]
pub enum MaintenanceCommand {
    Rekey(RekeyArgs),
    /// Export the complete configuration of a stopped source deployment.
    ExportConfig(ExportConfigArgs),
}

/// Read-only configuration export of a stopped source deployment.
///
/// The database URL and the master secret come from the node configuration or
/// the environment; the passphrase never appears on the command line.
#[derive(Debug, Args, Clone)]
#[command(group(
    ArgGroup::new("passphrase_source")
        .required(true)
        .multiple(false)
        .args(["passphrase_file", "passphrase_stdin"])
))]
pub struct ExportConfigArgs {
    /// Destination path of the encrypted archive; an existing file is never overwritten.
    #[arg(long)]
    pub out: std::path::PathBuf,
    /// File holding the archive passphrase (one trailing line ending is stripped).
    #[arg(long)]
    pub passphrase_file: Option<std::path::PathBuf>,
    /// Read the archive passphrase from standard input.
    #[arg(long)]
    pub passphrase_stdin: bool,
}

#[derive(Debug, Args, Clone, Default)]
pub struct RekeyArgs {
    #[arg(long)]
    pub apply: bool,
    #[arg(long)]
    pub ack_stop_old_nodes: bool,
    #[arg(long)]
    pub legacy_key_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum OpenapiCommand {
    Export(ExportOpenapiArgs),
}

#[derive(Debug, Args, Clone)]
pub struct ExportOpenapiArgs {
    #[arg(long, default_value = "openapi/admin-api.yaml")]
    pub out: String,
}

#[derive(Debug, Subcommand)]
pub enum CertCommand {
    Init(CertInitArgs),
}

#[derive(Debug, Args, Clone)]
pub struct CertInitArgs {
    #[arg(long)]
    pub host: String,
    #[arg(long, default_value = "./certs")]
    pub out: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn rekey_parser_defaults_to_dry_run_and_rejects_secret_flags() {
        let cli = Cli::parse_from(["prompt-ferry", "maintenance", "rekey"]);
        let Some(Command::Maintenance(MaintenanceCommand::Rekey(args))) = cli.command else {
            panic!("expected rekey command");
        };
        assert!(!args.apply && !args.ack_stop_old_nodes && args.legacy_key_file.is_none());
        for flag in ["--legacy-key", "--master-secret", "--database-url"] {
            assert!(
                Cli::try_parse_from(["prompt-ferry", "maintenance", "rekey", flag, "secret"])
                    .is_err()
            );
        }
        let cli = Cli::parse_from([
            "prompt-ferry",
            "maintenance",
            "rekey",
            "--apply",
            "--ack-stop-old-nodes",
            "--legacy-key-file",
            "keyfile",
        ]);
        let Some(Command::Maintenance(MaintenanceCommand::Rekey(args))) = cli.command else {
            panic!("expected rekey command")
        };
        assert!(args.apply && args.ack_stop_old_nodes);
        assert_eq!(
            args.legacy_key_file.as_deref(),
            Some(std::path::Path::new("keyfile"))
        );
    }

    #[test]
    fn export_config_requires_exactly_one_passphrase_source() {
        assert!(
            Cli::try_parse_from(["prompt-ferry", "maintenance", "export-config", "--out", "a"])
                .is_err(),
            "a passphrase source is required"
        );
        assert!(
            Cli::try_parse_from([
                "prompt-ferry",
                "maintenance",
                "export-config",
                "--out",
                "a",
                "--passphrase-file",
                "p",
                "--passphrase-stdin",
            ])
            .is_err(),
            "the passphrase sources are mutually exclusive"
        );
        let cli = Cli::parse_from([
            "prompt-ferry",
            "maintenance",
            "export-config",
            "--out",
            "a",
            "--passphrase-file",
            "p",
        ]);
        let Some(Command::Maintenance(MaintenanceCommand::ExportConfig(args))) = cli.command else {
            panic!("expected export-config command")
        };
        assert_eq!(args.out, std::path::Path::new("a"));
        assert_eq!(
            args.passphrase_file.as_deref(),
            Some(std::path::Path::new("p"))
        );
        assert!(!args.passphrase_stdin);

        let cli = Cli::parse_from([
            "prompt-ferry",
            "maintenance",
            "export-config",
            "--out",
            "a",
            "--passphrase-stdin",
        ]);
        let Some(Command::Maintenance(MaintenanceCommand::ExportConfig(args))) = cli.command else {
            panic!("expected export-config command")
        };
        assert!(args.passphrase_stdin && args.passphrase_file.is_none());
    }

    #[test]
    fn export_config_rejects_secret_and_url_flags() {
        for flag in [
            "--passphrase",
            "--database-url",
            "--master-secret",
            "--url",
            "--password",
        ] {
            assert!(
                Cli::try_parse_from([
                    "prompt-ferry",
                    "maintenance",
                    "export-config",
                    "--out",
                    "a",
                    "--passphrase-stdin",
                    flag,
                    "secret",
                ])
                .is_err(),
                "{flag} must not be accepted"
            );
        }
        assert!(
            Cli::try_parse_from([
                "prompt-ferry",
                "maintenance",
                "export-config",
                "--passphrase-stdin",
            ])
            .is_err(),
            "the output path is required"
        );
    }

    #[test]
    fn no_subcommand_selects_the_node_startup() {
        let cli = Cli::parse_from(["prompt-ferry"]);

        assert!(
            cli.command.is_none(),
            "an argument-free invocation must be the node entrypoint"
        );
        assert!(!cli.node.ack_stop_old_nodes);
    }

    #[test]
    fn node_acknowledgement_parses_without_a_subcommand() {
        let cli = Cli::parse_from(["prompt-ferry", "--ack-stop-old-nodes"]);

        assert!(cli.command.is_none());
        assert!(cli.node.ack_stop_old_nodes);
    }

    #[test]
    fn node_option_may_not_be_combined_with_a_subcommand() {
        // Mixing them would silently drop the flag, so the parser refuses it.
        Cli::try_parse_from(["prompt-ferry", "--ack-stop-old-nodes", "openapi", "export"])
            .expect_err("a node option must not be applied to another subcommand");
    }

    #[test]
    fn removed_component_commands_are_rejected() {
        for command in ["relay", "worker", "serve"] {
            Cli::try_parse_from(["prompt-ferry", command])
                .expect_err("the removed component command must not parse");
        }
    }

    #[test]
    fn removed_node_flags_are_rejected() {
        for flag in [
            "--internal-worker-bind",
            "--bind",
            "--master-secret",
            "--database-url",
            "--client-token",
            "--worker-token",
            "--worker-identity",
            "--bootstrap-admin-password",
            "--worker-config-encryption-key",
        ] {
            Cli::try_parse_from(["prompt-ferry", flag, "value"])
                .expect_err("the removed node flag must not parse");
        }
    }

    #[test]
    fn maintenance_and_utility_commands_stay_available() {
        let cli = Cli::parse_from(["prompt-ferry", "maintenance", "rekey"]);
        assert!(matches!(
            cli.command,
            Some(Command::Maintenance(MaintenanceCommand::Rekey(_)))
        ));

        let cli = Cli::parse_from(["prompt-ferry", "openapi", "export"]);
        assert!(matches!(
            cli.command,
            Some(Command::Openapi(OpenapiCommand::Export(ExportOpenapiArgs { out })))
                if out == "openapi/admin-api.yaml"
        ));

        let cli = Cli::parse_from(["prompt-ferry", "cert", "init", "--host", "example.test"]);
        assert!(matches!(
            cli.command,
            Some(Command::Cert(CertCommand::Init(CertInitArgs { host, out })))
                if host == "example.test" && out == "./certs"
        ));
    }
}
