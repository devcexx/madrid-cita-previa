use madrid_cita_previa::{AppointmentSession, StaticOffice};
use reqwest::ClientBuilder;
use std::ops::Deref;

use super::ExitCode;

#[derive(clap::Args)]
pub struct Args {
    /// Fetch appointments for this procedure.
    #[arg(short, long)]
    procedure_id: u32,

    /// Search only on this office
    #[arg(short, long)]
    office_id: Option<u32>,

    /// Search only in the offices within the given grooup
    #[arg(short = 'g', long)]
    office_group: Option<String>,
}

fn filter_offices<T: Deref<Target = &'static StaticOffice>>(
    mut offices: Vec<T>,
    match_office_id: Option<u32>,
    match_office_group: Option<String>,
) -> Vec<T> {
    if let Some(office_id) = match_office_id {
        offices.retain(|office| office.id.0 == office_id);
    }

    if let Some(group) = match_office_group {
        let search_group = group.to_lowercase();
        offices.retain(|office| office.group.to_lowercase() == search_group);
    }

    offices
}

pub async fn main(args: Args) -> anyhow::Result<ExitCode> {
    // Get all offices and apply filters
    let all_offices: Vec<_> = madrid_cita_previa_data::offices::ALL.iter().collect();
    let filtered_offices = filter_offices(all_offices, args.office_id, args.office_group);

    let offices_with_procedure = filtered_offices
        .into_iter()
        .filter_map(|office| {
            if let Some(proc) = office
                .procedures
                .iter()
                .find(|proc| proc.procedure_id.0 == args.procedure_id)
            {
                Some((office, proc))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if offices_with_procedure.is_empty() {
        eprintln!("No offices match the specified criteria.");
        return Ok(ExitCode::FaultOrArgsError);
    }

    let session = AppointmentSession::new(ClientBuilder::new());
    let mut found_appointments = false;

    for (office, procedure) in offices_with_procedure {
        let appointments = session
            .get_appointments_for_office(office.id, procedure.procedure_office_id)
            .await?;
        if !appointments.is_empty() {
            found_appointments = true;
        }
        println!("{}: {:?}", office.name, appointments);
    }

    if !found_appointments {
        eprintln!("No appointments found in any of the filtered offices.");
        Ok(ExitCode::RequestUnsatisfied)
    } else {
        Ok(ExitCode::Ok)
    }
}
