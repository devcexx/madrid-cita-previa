use std::process::Termination;

pub mod fetch_closest_appointment_office;
pub mod fetch_procedure_appointments;
pub mod list_offices;
pub mod list_procedures;
pub mod office_info;

#[repr(u8)]
pub enum ExitCode {
    Ok = 0,
    FaultOrArgsError = 1,
    RequestUnsatisfied = 2,
}

impl Termination for ExitCode {
    fn report(self) -> std::process::ExitCode {
        std::process::ExitCode::from(self as u8)
    }
}
