use super::ExitCode;

#[derive(clap::Args)]
pub struct Args {
    /// Office ID to get information about
    #[arg(short, long)]
    pub office_id: u32,
}

pub async fn main(args: Args) -> anyhow::Result<ExitCode> {
    let office = madrid_cita_previa_data::offices::ALL
        .iter()
        .find(|office| office.id.0 == args.office_id)
        .ok_or_else(|| anyhow::anyhow!("Office with ID {} not found", args.office_id))?;

    println!("Office Information:");
    println!(" - ID: {}", office.id.0);
    println!(" - Name: {}", office.name);
    println!(" - Group: {}", office.group);
    println!(" - Available Procedures:");

    for procedure in office.procedures {
        let proc_info = madrid_cita_previa_data::procedures::ALL
            .iter()
            .find(|p| p.procedure_id == procedure.procedure_id)
            .unwrap();

        println!(
            "  - {} (ID: {}; Procedure Office ID: {})",
            proc_info.procedure_name, procedure.procedure_id.0, procedure.procedure_office_id.0
        );
    }

    Ok(ExitCode::Ok)
}
