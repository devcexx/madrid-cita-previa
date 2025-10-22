use std::{collections::HashMap, str::FromStr, sync::Arc};

use anyhow::{Context, bail};
use chrono::{DateTime, NaiveDateTime, NaiveTime, TimeZone};
use chrono::{NaiveDate, Utc};
use lazy_static::lazy_static;
use log::{debug, trace};
use regex::Regex;
use reqwest::RequestBuilder;
use reqwest::{
    ClientBuilder, Response, Url,
    header::{ACCEPT, CONTENT_LENGTH, HeaderMap, HeaderValue, USER_AGENT},
};
use scraper::{Html, Selector};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use tokio::sync::Mutex;

use crate::{OfficeId, ProcedureId, ProcedureOfficeId};

#[derive(Default)]
struct SessionState {
    init: bool,
}

pub struct AppointmentSession {
    client: reqwest::Client,
    state: Arc<Mutex<SessionState>>,
}

lazy_static! {
    static ref BASE_URL: Url = Url::parse("https://servpub.madrid.es/GNSIS_WBCIUDADANO/").unwrap();
    static ref ENDPOINT_AJAX_AUTH: Url = BASE_URL.join("AjaxPantallaAcceso").unwrap();
    static ref ENDPOINT_CLOSEST_APPOINTMENT_OFFICE: Url =
        BASE_URL.join("oficinaCitaProxima.do").unwrap();
    static ref ENDPOINT_OFFICE_APPOINTMENTS: Url = BASE_URL.join("horarioOficina.do").unwrap();
    static ref ENDPOINT_APPOINTMENTS_BY_OFFICE_LANDING: Url = BASE_URL.join("oficina.do").unwrap();
    static ref ENDPOINT_APPOINTMENTS_BY_PROCEDURE_LANDING: Url =
        BASE_URL.join("tramite.do").unwrap();
    static ref ENDPOINT_DAY_APPOINTMENT_SLOTS: Url = BASE_URL.join("franjasDia.do").unwrap();
    static ref ENDPOINT_OFFICE_INFO: Url = BASE_URL.join("dameOficina.do").unwrap();
    static ref RE_AVAILABLE_APPOINTMENTS: Regex =
        Regex::new(r#"JSON\.parse\(\s*'([^']*)'\s*\)"#).unwrap();
    static ref SELECTOR_PROCEDURES_COMBOBOX: Selector =
        Selector::parse("select[id=selectTramites]").unwrap();
    static ref SELECTOR_OFFICES_COMBOBOX: Selector =
        Selector::parse("select[id=selectOficinas]").unwrap();
    static ref SELECTOR_OPTGROUP: Selector = Selector::parse("optgroup").unwrap();
    static ref SELECTOR_OPTION: Selector = Selector::parse("option").unwrap();
}

#[derive(Deserialize, Debug)]
pub struct NetOfficeProcedureModel {
    #[serde(rename = "categoria")]
    pub category: String,
    #[serde(rename = "nombreTramite")]
    pub name: String,
    #[serde(rename = "idTramite")]
    pub office_procedure_id: ProcedureOfficeId,
    #[serde(rename = "idFamiliaCita")]
    pub procedure_id: ProcedureId,
}

#[derive(Deserialize, Debug)]
pub struct NetOfficeModel {
    #[serde(rename = "idOficina")]
    pub office_id: u32,
    #[serde(rename = "codIntegracion")]
    pub office_code: Option<String>,
    #[serde(rename = "latitud")]
    pub latitude: f64,
    #[serde(rename = "longitud")]
    pub longitude: f64,
    #[serde(rename = "nombreOficina")]
    pub name: String,
    #[serde(rename = "direccion")]
    pub address: String,
    #[serde(rename = "codigoDistrito")]
    pub district_code: String,
    #[serde(rename = "nombreDistrito")]
    pub district_name: String,
    #[serde(rename = "urlInformacion")]
    pub url: String,
    #[serde(rename = "tramites")]
    pub procedures: Vec<NetOfficeProcedureModel>,
}

#[derive(Debug)]
pub struct NetOfficeBasicInfoModel {
    pub name: String,
    pub group: String,
    pub id: OfficeId,
}

#[derive(Deserialize, Debug)]
pub struct NetAppointment {
    #[serde(rename = "dia")]
    pub day: u8,
    #[serde(rename = "mes")]
    pub month: u8,
    #[serde(rename = "ano")]
    pub year: u32,
}

pub struct NetProcedureModel {
    pub procedure_category: String,
    pub procedure_name: String,
    pub procedure_id: ProcedureId,
}

#[derive(Deserialize, Debug)]
pub struct NetAppointmentSlot {
    #[serde(rename = "hora")]
    raw_time: String,

    #[serde(rename = "disponible")]
    available: bool,
}

#[derive(Deserialize, Debug)]
pub struct NetAppointmentSlotSet {
    #[serde(rename = "huecos")]
    slots: Vec<NetAppointmentSlot>,
}

#[derive(Deserialize, Debug)]
pub struct NetAppointmentHourlySlots {
    #[serde(rename = "franjasMinuto")]
    slot_sets: Vec<NetAppointmentSlotSet>,
}

impl AppointmentSession {
    pub fn new(cb: ClientBuilder) -> Self {
        let mut headers = HeaderMap::new();
        headers.append(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (X11; Linux x86_64; rv:141.0) Gecko/20100101 Firefox/141.0",
            ),
        );
        let client = cb
            .cookie_store(true)
            .default_headers(headers)
            .build()
            .unwrap();

        AppointmentSession {
            client,
            state: Arc::new(Mutex::new(SessionState::default())),
        }
    }

    pub async fn ensure_init(&self) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;
        if !state.init {
            self.init_session().await.context("Init session")?;
            self.auth_anonymous().await.context("Anonynous Auth")?;
            state.init = true;
        }
        Ok(())
    }

    async fn init_session(&self) -> anyhow::Result<()> {
        Self::trace_send_request(self.client.get(BASE_URL.clone()))
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn auth_anonymous(&self) -> anyhow::Result<()> {
        Self::trace_send_request(
            self.client
                .post(ENDPOINT_AJAX_AUTH.clone())
                .body("")
                .header(CONTENT_LENGTH, 0), // Must send always the Content-Length and set it to zero.
        )
        .await
        .with_context(|| format!("POST request to {}", ENDPOINT_AJAX_AUTH.as_str()))?
        .error_for_status()?;
        Ok(())
    }

    async fn trace_body_and_error_for_response(
        operation: &str,
        resp: Response,
    ) -> anyhow::Result<String> {
        debug!("{} status code: {}", operation, resp.status());
        let body = resp.text().await.context("Reading response body")?;

        trace!("{} response body: {}", operation, body);
        Ok(body)
    }

    fn trace_send_request(
        request: RequestBuilder,
    ) -> impl Future<Output = Result<Response, reqwest::Error>> {
        log::trace!("Sending request: {:?}", &request);
        return request.send();
    }

    async fn trace_json_body_and_error_for_response<T: for<'de> serde::de::Deserialize<'de>>(
        operation: &str,
        resp: Response,
    ) -> anyhow::Result<T> {
        let body = Self::trace_body_and_error_for_response(operation, resp).await?;
        Ok(serde_json::from_str(&body).context("Parsing response body")?)
    }

    fn read_office_optional<'a, T: DeserializeOwned>(body: &'a str) -> anyhow::Result<Option<T>> {
        let read = serde_json::from_str::<Map<String, Value>>(body)?;
        let Some(office_id) = read.get("idOficina") else {
            bail!("Required office id not present in json");
        };

        let Some(office_id_num) = office_id.as_u64() else {
            bail!("Office id is not a valid numeric value");
        };

        if office_id_num == 0 {
            Ok(None)
        } else {
            Ok(Some(serde_json::from_value(Value::Object(read))?))
        }
    }

    pub async fn get_office_closest_appointment(
        &self,
        procedure: ProcedureId,
    ) -> anyhow::Result<Option<NetOfficeModel>> {
        self.ensure_init().await?;

        let resp = Self::trace_send_request(
            self.client
                .post(ENDPOINT_CLOSEST_APPOINTMENT_OFFICE.clone())
                .header(ACCEPT, "application/json")
                .form(&HashMap::from([("idTipoTramite", procedure.0)])),
        )
        .await?;

        let body =
            Self::trace_body_and_error_for_response("get_office_closest_appointment", resp).await?;
        Ok(Self::read_office_optional(&body)?)
    }

    pub async fn get_appointments_for_office(
        &self,
        office: OfficeId,
        procedure_office_id: ProcedureOfficeId,
    ) -> anyhow::Result<Vec<NaiveDate>> {
        self.ensure_init().await?;

        let id_office = office.0.to_string();
        let id_procedure = procedure_office_id.0.to_string();

        let request = HashMap::from([
            ("valido", "true"),
            ("ruta", "oficina"),
            ("idCiudadanoCitaAnterior", ""),
            ("idOficinaEdicion", ""),
            ("idServicioEdicion", ""),
            ("usaVariablesEdicion", ""),
            ("esModificacion", ""),
            ("origen", ""),
            ("idFamiliaCita", ""),
            ("idOficina", &id_office),
            ("idServicio", &id_procedure),
            ("idTipoDocumentoUsuarioAut", ""),
            ("numeroDocumento", ""),
        ]);

        let resp = Self::trace_send_request(
            self.client
                .post(ENDPOINT_OFFICE_APPOINTMENTS.clone())
                .form(&request),
        )
        .await?;

        let body =
            Self::trace_body_and_error_for_response("get_appointments_for_office", resp).await?;
        if body.contains("Las citas disponibles en esta oficina han sido reservadas recientemente")
        {
            return Ok(Vec::new());
        }

        if let Some(caps) = RE_AVAILABLE_APPOINTMENTS.captures(&body) {
            let appointments = serde_json::from_str::<Vec<NetAppointment>>(&caps[1])?;
            return Ok(appointments
                .into_iter()
                .map(|app| {
                    NaiveDate::from_ymd_opt(app.year as i32, app.month as u32, app.day as u32)
                        .unwrap()
                })
                .collect());
        } else {
            bail!("Response body didn't match expected response for get_appointments_for_office")
        }
    }

    pub async fn get_available_appointment_slots_for_office_day(
        &self,
        procedure_office_id: ProcedureOfficeId,
        day: NaiveDate,
    ) -> anyhow::Result<impl Iterator<Item = DateTime<chrono_tz::Tz>>> {
        fn build_slot_dt(raw_time: &str, day: NaiveDate) -> DateTime<chrono_tz::Tz> {
            let time = NaiveTime::parse_from_str(raw_time, "%H:%M").unwrap();
            chrono_tz::Tz::from_local_datetime(
                &chrono_tz::Europe::Madrid,
                &NaiveDateTime::new(day, time),
            )
            .unwrap()
        }

        self.ensure_init().await?;
        let id_procedure = procedure_office_id.0.to_string();
        let current_ts = Utc::now().timestamp_millis().to_string();
        let search_day = day.format("%d/%m/%Y").to_string();

        let req = self
            .client
            .get(ENDPOINT_DAY_APPOINTMENT_SLOTS.clone())
            .query(&[
                ("idTramite", id_procedure.as_str()),
                ("dia", &search_day),
                ("esUltimoDiaHuecos", "false"),
                ("nh", "0"),
                ("time", &current_ts),
                ("idTipoAtencion", "1"),
            ]);
        let resp = Self::trace_send_request(req).await?;

        let hourly_slots = Self::trace_json_body_and_error_for_response::<
            Vec<NetAppointmentHourlySlots>,
        >("get_available_appointment_slots_for_office_day", resp)
        .await?;
        Ok(hourly_slots
            .into_iter()
            .flat_map(|hourly_slots| hourly_slots.slot_sets)
            .flat_map(|minute_slots| minute_slots.slots)
            .filter(|slot| slot.available)
            .map(move |slot| build_slot_dt(&slot.raw_time, day)))
    }

    pub async fn list_available_procedures(&self) -> anyhow::Result<Vec<NetProcedureModel>> {
        self.ensure_init().await?;

        let resp = Self::trace_send_request(
            self.client
                .get(ENDPOINT_APPOINTMENTS_BY_PROCEDURE_LANDING.clone()),
        )
        .await?;

        let body =
            Self::trace_body_and_error_for_response("list_available_procedures", resp).await?;

        // TODO remove unwraps
        let html = Html::parse_document(&body);
        let select = html.select(&SELECTOR_PROCEDURES_COMBOBOX).next().unwrap();

        Ok(select
            .select(&SELECTOR_OPTGROUP)
            .flat_map(|optgroup| {
                let label = optgroup.attr("label").unwrap();
                debug!("list_available_procedures: Group: {}", label);
                optgroup.select(&SELECTOR_OPTION).map(|option| {
                    let procedure_name = option.inner_html();
                    let procedure_id = option.attr("value").unwrap();
                    debug!(
                        "list_available_procedures:   Procedure: {}; {}",
                        procedure_name, procedure_id
                    );

                    NetProcedureModel {
                        procedure_category: label.to_string(),
                        procedure_name,
                        procedure_id: ProcedureId(FromStr::from_str(procedure_id).unwrap()),
                    }
                })
            })
            .collect())
    }

    pub async fn list_offices(&self) -> anyhow::Result<Vec<NetOfficeBasicInfoModel>> {
        self.ensure_init().await?;

        let resp = Self::trace_send_request(
            self.client
                .get(ENDPOINT_APPOINTMENTS_BY_OFFICE_LANDING.clone()),
        )
        .await?;

        let body = Self::trace_body_and_error_for_response("list_offices", resp).await?;

        // TODO remove unwraps

        let r = Html::parse_document(&body);

        let select = r.select(&SELECTOR_OFFICES_COMBOBOX).next().unwrap();

        Ok(select
            .select(&SELECTOR_OPTGROUP)
            .flat_map(|optgroup| {
                let office_category_name = optgroup.attr("label").unwrap();
                optgroup.select(&SELECTOR_OPTION).map(|option| {
                    let office_name = option.inner_html();
                    let office_id = option.attr("value").unwrap();
                    NetOfficeBasicInfoModel {
                        name: office_name,
                        group: office_category_name.to_string(),
                        id: OfficeId(FromStr::from_str(office_id).unwrap()),
                    }
                })
            })
            .collect())
    }

    pub async fn get_office_details(
        &self,
        office_id: OfficeId,
    ) -> anyhow::Result<Option<NetOfficeModel>> {
        let office_id_str = office_id.0.to_string();

        let resp = Self::trace_send_request(
            self.client
                .get(ENDPOINT_OFFICE_INFO.clone())
                .query(&HashMap::from([("idOficina", office_id_str)])),
        )
        .await?;

        let body = Self::trace_body_and_error_for_response("get_office_details", resp).await?;
        Ok(Self::read_office_optional(&body)?)
    }
}
