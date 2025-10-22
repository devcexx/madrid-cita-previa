use chrono::{DateTime, NaiveDate};
use madrid_cita_previa::{AppointmentSession, StaticOffice};
use reqwest::ClientBuilder;
use serde::Serialize;
use std::ops::Deref;

use super::ExitCode;

#[derive(clap::Args)]
pub struct Args {
    /// Fetch appointments for this procedure.
    #[arg(short, long)]
    procedure_id: u32,

    /// Fetch also slots for each day found to have appointments.
    #[arg(short, long)]
    slots: bool,

    /// Search only on this office
    #[arg(short, long)]
    office_id: Option<u32>,

    /// Search only in the offices within the given group
    #[arg(short = 'g', long)]
    office_group: Option<String>,

    /// Prints all the results at once in JSON format
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
pub struct DayWithAppointments {
    day: String,

    // May be null if no slots download has been requested with --slots parameter.
    slots: Option<Vec<i64>>,
}

#[derive(Serialize)]
pub struct OfficeBasicInfo {
    office_id: u32,
    office_name: &'static str,
}

#[derive(Serialize)]
pub struct OfficeAppoinmentsInfo {
    office: OfficeBasicInfo,
    appointments: Vec<DayWithAppointments>,
}

#[derive(Serialize)]
pub struct ProcudureAppointments {
    appointments_by_office: Vec<OfficeAppoinmentsInfo>,
}

pub fn basic_office_info_from_static(office: &StaticOffice) -> OfficeBasicInfo {
    OfficeBasicInfo {
        office_id: office.id.0,
        office_name: office.name,
    }
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
    let mut acc_appointments: Vec<OfficeAppoinmentsInfo> = Vec::new();

    for (office, procedure) in offices_with_procedure {
        let appointments = session
            .get_appointments_for_office(office.id, procedure.procedure_office_id)
            .await?;

        let mut slots_by_day: Vec<(NaiveDate, Option<Vec<DateTime<chrono_tz::Tz>>>)> = Vec::new();
        // Sometimes it may happen that get_appointments_for_office reports a
        // day with appointments, but then no slots are avaiable. For such
        // reason, setting the found_appointments differently depending on
        // what data are being asked to get.
        if args.slots {
            for appointment_day in appointments.into_iter() {
                let slots_for_current_day: Vec<DateTime<chrono_tz::Tz>> = session
                    .get_available_appointment_slots_for_office_day(
                        procedure.procedure_office_id,
                        appointment_day,
                    )
                    .await?
                    .collect();

                if !slots_for_current_day.is_empty() {
                    found_appointments = true;
                    // Only register this day as a "day with appointments" if we
                    // actually found any slot in the returned day.
                    slots_by_day.push((appointment_day, Some(slots_for_current_day)));
                }
            }

            if !args.json {
                println!(
                    "{}: {:?}",
                    office.name,
                    slots_by_day
                        .iter()
                        .flat_map(|(_, slot)| slot)
                        .collect::<Vec<_>>()
                );
            }
        } else {
            if !appointments.is_empty() {
                found_appointments = true;
            }

            slots_by_day.extend(appointments.iter().map(|day| (*day, None)));
            if !args.json {
                println!("{}: {:?}", office.name, appointments);
            }
        }

        if args.json {
            acc_appointments.push(OfficeAppoinmentsInfo {
                office: basic_office_info_from_static(office),
                appointments: slots_by_day
                    .iter()
                    .map(|(day, slots)| DayWithAppointments {
                        day: day.to_string(),
                        slots: match slots {
                            None => None,
                            Some(slots) => {
                                Some(slots.iter().map(|slot| slot.timestamp()).collect())
                            }
                        },
                    })
                    .collect(),
            });
        }
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&ProcudureAppointments {
                appointments_by_office: acc_appointments
            })
            .unwrap()
        );
    }

    if !found_appointments {
        if !args.json {
            eprintln!("No appointments found in any of the filtered offices.");
        }
        Ok(ExitCode::RequestUnsatisfied)
    } else {
        Ok(ExitCode::Ok)
    }
}
