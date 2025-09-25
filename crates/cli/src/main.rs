use log::{LevelFilter, info};
use madrid_cita_previa::AppointmentSession;
use reqwest::ClientBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .parse_default_env()
        .init();

    //let procedure = cita_previa_util_data::procedures::;
    //
    let procedure = madrid_cita_previa_data::procedures::ALL
        .iter()
        .filter(|proc| proc.procedure_name == "Altas, bajas y cambio de domicilio en Padrón")
        .next()
        .unwrap();

    let offices_with_procedure = madrid_cita_previa_data::offices::ALL
        .iter()
        .filter_map(|office| {
            let proc = office
                .procedures
                .iter()
                .find(|proc| proc.procedure_id == procedure.procedure_id);
            if let Some(proc) = proc {
                Some((office, proc))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let sess = AppointmentSession::new(ClientBuilder::new());
    for (office, proc) in offices_with_procedure {
        let app = sess
            .get_appointments_for_office(office.id, proc.procedure_office_id)
            .await?;
        info!("Appointments for {}: {:?}", office.name, app);
    }

    Ok(())
}
