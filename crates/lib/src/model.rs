use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// The unique numeric ID of an office.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfficeId(pub u32);

/// The unique numeric ID of a procedure.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcedureId(pub u32);

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcedureOfficeId(pub u32);


#[derive(Deserialize, Serialize, Debug)]
pub struct DataGenModel {
    pub offices: Vec<DataGenOffice>,
    pub procedures: Vec<DataGenProcedure>
}

#[derive(Deserialize, Serialize, Debug)]
pub struct DataGenOffice {
    pub name: String,
    pub group: String,
    pub id: OfficeId,
    pub procedures: Vec<DataGenOfficeProcedure>
}

#[derive(Deserialize, Serialize, Debug)]
pub struct DataGenOfficeProcedure {
    pub procedure_name: String,
    pub procedure_category: String,
    pub procedure_office_id: ProcedureOfficeId,
    pub procedure_id: ProcedureId
}

#[derive(Deserialize, Serialize, Debug)]
pub struct DataGenProcedure {
    pub procedure_category: String,
    pub procedure_name: String,
    pub procedure_id: ProcedureId
}


#[derive(Debug)]
pub struct StaticOffice {
    pub name: &'static str,
    pub group: &'static str,
    pub id: OfficeId,
    pub procedures: &'static [StaticOfficeProcedure]
}

#[derive(Debug)]
pub struct StaticOfficeProcedure {
    pub procedure_name: &'static str,
    pub procedure_category: &'static str,
    pub procedure_office_id: ProcedureOfficeId,
    pub procedure_id: ProcedureId
}

#[derive(Debug)]
pub struct StaticProcedure {
    pub procedure_category: &'static str,
    pub procedure_name: &'static str,
    pub procedure_id: ProcedureId
}
