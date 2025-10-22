use std::{io::Write, pin::Pin, process::ExitCode};

use anyhow::anyhow;
use clap::Parser;
use log::{LevelFilter, info};
use madrid_cita_previa::{
    AppointmentSession, DataGenModel, DataGenOffice, DataGenOfficeProcedure, DataGenProcedure,
    NetOfficeBasicInfoModel, NetOfficeModel,
};
use reqwest::ClientBuilder;
use tokio::{
    fs::File,
    io::{AsyncWrite, AsyncWriteExt, stdout},
};

#[derive(Parser)]
struct Args {
    /// Prints to the standard output the available offices and procedures,
    /// without downloading or saving further details, and exits.
    #[arg(long)]
    list_only: bool,

    /// Download only data for the offices in the given group
    #[arg(long)]
    filter_group: Option<String>,

    /// Download only data for the offices whose name contains the given string
    #[arg(long)]
    filter_name: Option<String>,

    /// Output file for the downloaded models. Defaults to the standard output ("-")
    #[arg(short, long, default_value = "-")]
    output: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    main0().await
}

pub fn filter_relevant_offices(
    offices: &mut Vec<NetOfficeBasicInfoModel>,
    group_filter: Option<String>,
    name_filter: Option<String>,
) {
    if let Some(group) = group_filter {
        let group = group.to_lowercase();
        offices.retain(|office| {
            let keep = office.group.to_lowercase() == group;
            if !keep {
                info!("Filtering office out: {}", office.name);
            }

            keep
        });
    }

    if let Some(name) = name_filter {
        let name = name.to_lowercase();
        offices.retain(|office| {
            let keep = office.name.to_lowercase().contains(&name);
            if !keep {
                info!("Filtering office out: {}", office.name);
            }
            keep
        });
    }
}

async fn main0() -> anyhow::Result<ExitCode> {
    let args = Args::parse();

    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .parse_default_env()
        .init();
    let session = AppointmentSession::new(ClientBuilder::new());

    info!("Listing offices...");
    let mut offices = session.list_offices().await?;
    info!("Listing procedures...");
    let procs = session.list_available_procedures().await?;

    if args.list_only {
        println!("Offices:");
        println!("{:<5} | {:<40} | {}", "ID", "Group", "Name");
        for office in offices.into_iter() {
            println!(
                "{:<5} | {:<40} | {}",
                office.id.0, office.group, office.name
            );
        }
        println!();
        println!("Procedures:");
        println!("{:<5} | {:<40} | {}", "ID", "Category", "Name");
        for proc in procs.into_iter() {
            println!(
                "{:<5} | {:<40} | {}",
                proc.procedure_id.0, proc.procedure_category, proc.procedure_name
            );
        }
        return Ok(ExitCode::SUCCESS);
    }
    filter_relevant_offices(&mut offices, args.filter_group, args.filter_name);

    let mut office_data: Vec<(NetOfficeBasicInfoModel, NetOfficeModel)> = Vec::new();
    for office in offices.into_iter() {
        let id = office.id;
        info!("Downloading office info: {}", &office.name);
        let details = session
            .get_office_details(id)
            .await?
            .ok_or_else(|| anyhow!("Couldn't download office info for office {:?}", &office))?;
        office_data.push((office, details));
    }

    let model = DataGenModel {
        offices: office_data
            .into_iter()
            .map(|(office_basic, office)| DataGenOffice {
                name: office.name,
                group: office_basic.group,
                id: office_basic.id,
                procedures: office
                    .procedures
                    .into_iter()
                    .map(|proc| DataGenOfficeProcedure {
                        procedure_name: proc.name,
                        procedure_category: proc.category,
                        procedure_office_id: proc.office_procedure_id,
                        procedure_id: proc.procedure_id,
                    })
                    .collect(),
            })
            .collect(),
        procedures: procs
            .into_iter()
            .map(|proc| DataGenProcedure {
                procedure_category: proc.procedure_category,
                procedure_name: proc.procedure_name,
                procedure_id: proc.procedure_id,
            })
            .collect(),
    };

    let str = serde_json::to_string(&model)?;
    let mut writer: Pin<Box<dyn AsyncWrite>>;
    if args.output == "-" {
        writer = Box::pin(stdout());
    } else {
        writer = Box::pin(File::create(args.output).await?)
    }
    writer.write_all(str.as_bytes()).await?;
    Ok(ExitCode::SUCCESS)
}
