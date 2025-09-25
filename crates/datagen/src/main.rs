use log::{LevelFilter, info};
use madrid_cita_previa::{
    AppointmentSession, DataGenModel, DataGenOffice, DataGenOfficeProcedure, DataGenProcedure,
    NetOfficeBasicInfoModel, NetOfficeModel, OfficeId,
};
use reqwest::ClientBuilder;
use tokio::{fs::File, io::AsyncWriteExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    main0().await
}

struct DataGenOfficBasicInfo {
    pub name: String,
    pub group: String,
    pub id: OfficeId,
}

pub fn filter_relevant_offices(
    offices: Vec<NetOfficeBasicInfoModel>,
) -> Vec<NetOfficeBasicInfoModel> {
    offices
    //  offices.into_iter().filter(|office| office.name.contains("URB")).collect()
}

async fn main0() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .parse_default_env()
        .init();
    let session = AppointmentSession::new(ClientBuilder::new());

    info!("Listing offices...");
    let offices = filter_relevant_offices(session.list_offices().await?);
    info!("Listing procedures...");
    let procs = session.list_available_procedures().await?;

    let mut office_data: Vec<(NetOfficeBasicInfoModel, NetOfficeModel)> = Vec::new();
    for office in offices.into_iter() {
        let id = office.id;
        info!("Downloading office info: {}", &office.name);
        office_data.push((office, session.get_office_details(id).await?.unwrap()));
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

    let str = serde_json::to_string(&model).unwrap();
    File::create("model2.json")
        .await
        .unwrap()
        .write_all(&str.as_bytes())
        .await
        .unwrap();
    Ok(())
}
