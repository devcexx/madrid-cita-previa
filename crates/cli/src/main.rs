use std::process::exit;

use clap::{Parser, Subcommand};
use log::LevelFilter;
use madrid_cita_previa::NetAppointmentHourlySlots;
mod commands;

#[derive(Parser)]
#[command(name = "madrid-cita-previa")]
#[command(about = "A CLI tool for querying Madrid available procedure appointments")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    subcommand: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List available offices
    ListOffices(commands::list_offices::Args),
    /// List available procedures
    ListProcedures(commands::list_procedures::Args),
    /// Get information about a specific office
    OfficeInfo(commands::office_info::Args),
    /// Find the office with the closest available appointment
    FetchClosestAppointmentOffice(commands::fetch_closest_appointment_office::Args),
    /// Find the appointments for a given procedure
    FetchProcedureAppointments(commands::fetch_procedure_appointments::Args),
}

#[tokio::main]
async fn main() -> anyhow::Result<commands::ExitCode> {
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .parse_default_env()
        .init();

    let cli = Cli::parse();

    Ok(match cli.subcommand {
        Commands::ListOffices(args) => commands::list_offices::main(args).await?,
        Commands::ListProcedures(args) => commands::list_procedures::main(args).await?,
        Commands::OfficeInfo(args) => commands::office_info::main(args).await?,
        Commands::FetchClosestAppointmentOffice(args) => {
            commands::fetch_closest_appointment_office::main(args).await?
        }
        Commands::FetchProcedureAppointments(args) => {
            commands::fetch_procedure_appointments::main(args).await?
        }
    })
}
