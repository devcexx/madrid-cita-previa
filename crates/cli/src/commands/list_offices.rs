use std::ops::Deref;

use madrid_cita_previa::StaticOffice;

use super::ExitCode;

#[derive(clap::Args)]
pub struct Args {
    /// Filter offices by group name
    #[arg(short, long)]
    pub group: Option<String>,

    /// Filter offices by those that can handle the given procedure
    #[arg(short, long)]
    pub procedure: Option<u32>,
}

fn print_offices<T: Deref<Target = &'static StaticOffice>>(
    mut offices: Vec<T>,
    filter_by_group: Option<String>,
    filter_by_procedure: Option<u32>,
) {
    if let Some(group) = filter_by_group {
        let search_group = group.to_lowercase();

        offices.retain(|office| office.group.to_lowercase() == search_group);
    }

    if let Some(procedure) = filter_by_procedure {
        offices.retain(|office| {
            office
                .procedures
                .iter()
                .any(|proc| proc.procedure_id.0 == procedure)
        });
    }

    println!("{:<5} | {:<40} | {}", "ID", "Group", "Name");
    offices.sort_by(|a, b| Ord::cmp(a.group, b.group).then(Ord::cmp(a.name, b.name)));

    for office in offices {
        println!(
            "{:<5} | {:<40} | {}",
            office.id.0, office.group, office.name
        )
    }
}

pub async fn main(args: Args) -> anyhow::Result<ExitCode> {
    print_offices(
        madrid_cita_previa_data::offices::ALL.iter().collect(),
        args.group,
        args.procedure,
    );
    Ok(ExitCode::Ok)
}
