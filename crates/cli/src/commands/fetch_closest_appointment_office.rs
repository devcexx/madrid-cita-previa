use madrid_cita_previa::AppointmentSession;
use reqwest::ClientBuilder;

use super::ExitCode;

#[derive(clap::Args)]
pub struct Args {
    /// Procedure ID to find appointments for
    #[arg(short, long)]
    pub procedure_id: u32,
}

pub async fn main(args: Args) -> anyhow::Result<ExitCode> {
    let Some(procedure) = madrid_cita_previa_data::procedures::ALL
        .iter()
        .find(|proc| proc.procedure_id.0 == args.procedure_id)
    else {
        eprintln!("Unknown procedure: {}", args.procedure_id);
        return Ok(ExitCode::FaultOrArgsError);
    };

    eprintln!("Selected procedure: {}", procedure.procedure_name);
    let sess = AppointmentSession::new(ClientBuilder::new());
    let office = sess
        .get_office_closest_appointment(procedure.procedure_id)
        .await?;

    if let Some(office) = office {
        println!("{}", office.name);
    } else {
        return Ok(ExitCode::RequestUnsatisfied);
    }
    Ok(ExitCode::Ok)
}
