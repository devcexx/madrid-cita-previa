use super::ExitCode;

#[derive(clap::Args)]
pub struct Args {
    /// Filter procedures by category
    #[arg(short, long)]
    pub category: Option<String>,
}

pub async fn main(args: Args) -> anyhow::Result<ExitCode> {
    let procedures = if let Some(filter) = args.category {
        let filter_category = filter.to_lowercase();

        madrid_cita_previa_data::procedures::ALL
            .iter()
            .filter(|proc| proc.procedure_category.to_lowercase() == filter_category)
            .collect::<Vec<_>>()
    } else {
        madrid_cita_previa_data::procedures::ALL.iter().collect()
    };
    println!("{:<5} | {:<40} | {}", "ID", "Category", "Name");

    for procedure in procedures {
        println!(
            "{:<5} | {:<40} | {}",
            procedure.procedure_id.0, procedure.procedure_category, procedure.procedure_name
        );
    }

    Ok(ExitCode::Ok)
}
