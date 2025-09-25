use std::{collections::HashMap, str::FromStr, sync::Arc};

use anyhow::{Context, bail};
use chrono::NaiveDate;
use lazy_static::lazy_static;
use log::{debug, trace};
use regex::Regex;
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
    static ref ENDPOINT_CLOSEST_APPOINTMENT_OFFICE: Url = BASE_URL.join("oficinaCitaProxima.do").unwrap();
    static ref ENDPOINT_OFFICE_APPOINTMENTS: Url = BASE_URL.join("horarioOficina.do").unwrap();
    static ref ENDPOINT_APPOINTMENTS_BY_OFFICE_LANDING: Url = BASE_URL.join("oficina.do").unwrap();
    static ref ENDPOINT_APPOINTMENTS_BY_PROCEDURE_LANDING: Url = BASE_URL.join("tramite.do").unwrap();
    static ref ENDPOINT_OFFICE_INFO: Url = BASE_URL.join("dameOficina.do").unwrap();
    static ref RE_AVAILABLE_APPOINTMENTS: Regex = Regex::new(r#"JSON\.parse\(\s*'([^']*)'\s*\)"#).unwrap();

    static ref SELECTOR_PROCEDURES_COMBOBOX: Selector = Selector::parse("select[id=selectTramites]").unwrap();
    static ref SELECTOR_OFFICES_COMBOBOX: Selector = Selector::parse("select[id=selectOficinas]").unwrap();
    static ref SELECTOR_OPTGROUP: Selector = Selector::parse("optgroup").unwrap();
    static ref SELECTOR_OPTION: Selector = Selector::parse("option").unwrap();

    //static ref ENDPOINT_OFFICE: Url = Url::parse("https://servpub.madrid.es/GNSIS_WBCIUDADANO/").unwrap();
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
        self.client
            .get(BASE_URL.clone())
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn auth_anonymous(&self) -> anyhow::Result<()> {
        self.client
            .post(ENDPOINT_AJAX_AUTH.clone())
            .body("")
            .header(CONTENT_LENGTH, 0) // Must send always the Content-Length and set it to zero.
            .send()
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

        let resp = self
            .client
            .post(ENDPOINT_CLOSEST_APPOINTMENT_OFFICE.clone())
            .header(ACCEPT, "application/json")
            .form(&HashMap::from([("idTipoTramite", procedure.0)]))
            .send()
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

        let resp = self
            .client
            .post(ENDPOINT_OFFICE_APPOINTMENTS.clone())
            .form(&request)
            .send()
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

    pub async fn list_available_procedures(&self) -> anyhow::Result<Vec<NetProcedureModel>> {
        self.ensure_init().await?;

        let resp = self
            .client
            .get(ENDPOINT_APPOINTMENTS_BY_PROCEDURE_LANDING.clone())
            .send()
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

        let resp = self
            .client
            .get(ENDPOINT_APPOINTMENTS_BY_OFFICE_LANDING.clone())
            .send()
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

        let resp = self
            .client
            .get(ENDPOINT_OFFICE_INFO.clone())
            .query(&HashMap::from([("idOficina", office_id_str)]))
            .send()
            .await?;

        let body = Self::trace_body_and_error_for_response("get_office_details", resp).await?;
        Ok(Self::read_office_optional(&body)?)
    }
}

// {"idGrupoMaestro":128,"idOficina":128,"latitud":0.0,"longitud":0.0,"mensajeAviso":null,"nombreOficina":"OAC Fuencarral","tramites":[{"camposAdicionales":[],"categoria":"Padrón y censo","codigo360":"","codigoIntegracion":"","entidad":null,"idTipoServicio":22,"idTramite":1009,"avisoPublico":null,"nombreTramite":"Gestiones Padrón municipal","oficina":"OAC Fuencarral","urlInformacion":"https://madrid.es/go/padron","familiaCita":"Altas, bajas y cambio de domicilio en Padrón","idFamiliaCita":321,"avisoFamilia":"<p>\r\n\tConsulte la <strong>apertura de agendas </strong>en <a href=\"https://www.madrid.es/portales/munimadrid/es/Inicio/El-Ayuntamiento/Atencion-a-la-ciudadania/Oficinas-de-Atencion-a-la-Ciudadania/?vgnextfmt=default&amp;vgnextchannel=5b99cde2e09a4310VgnVCM1000000b205a0aRCRD\">Oficinas de Atención a la Ciudadanía</a>. \r\n</p>\r\n<p>\r\n\t<strong>Solo se necesita una cita</strong> para realizar todas las gestiones del mismo domicilio. \r\n</p>\r\n<p>\r\n\tEn<strong> altas y cambios de domicilio, presente solicitud firmada por todos los solicitantes</strong> mayores de edad, <strong>documentos de identidad en vigor, documentación acreditativa del uso de la vivienda y autorizaciones necesarias</strong>. En <strong>bajas por fallecimiento</strong>, aporte <strong>certificado de defunción</strong> del registro civil. \r\n</p>\r\n<p>\r\n\tConsulte la documentación necesaria en <a href=\"https://www.madrid.es/portales/munimadrid/es/Inicio/Buscador/Padron-y-Censo-Electoral/?vgnextfmt=default&amp;vgnextoid=3da7968042610810VgnVCM1000001d4a900aRCRD&amp;vgnextchannel=7db8fc12aa936610VgnVCM1000008a4a900aRCRD\">Trámites de Padrón y censo electoral</a>, así como otra vía para realizar el trámite si se trata de&nbsp;<a href=\"https://sede.madrid.es/portal/site/tramites/menuitem.62876cb64654a55e2dbd7003a8a409a0/?vgnextoid=1a7e9374bcaed010VgnVCM1000000b205a0aRCRD&amp;vgnextchannel=b878a38813180210VgnVCM100000c90da8c0RCRD&amp;vgnextfmt=default\"> Modificación y actualización de datos personales excepto domicilio.</a>\r\n</p>","ocultoPublico":false,"idOficina":128,"codigoOculto":"","canalPrivado":false,"integracionBDC":false,"codigoURL":null,"idOpcionCitacion":0,"comprobacionPreviaWeb":true,"integracionSUPRA":false},{"camposAdicionales":[],"categoria":"Identificación Electrónica","codigo360":"","codigoIntegracion":"","entidad":null,"idTipoServicio":1541,"idTramite":3850,"avisoPublico":null,"nombreTramite":"Certificado FNMT","oficina":"OAC Fuencarral","urlInformacion":"https://madrid.es/go/HJqR","familiaCita":"Certificado electrónico FNMT Persona física","idFamiliaCita":1101,"avisoFamilia":"<p>\r\n\t<strong>Certificado electrónico</strong>, para la acreditación de identidad para la obtención del certificado electrónico FNMT es <strong>imprescindible solicitar previamente en la sede electrónica de la FNMT el código de solicitud y aportarlo</strong>, junto al documento de identidad, el día que acuda a la cita. Si requiere ayuda por motivo de discapacidad, acuda al punto central de la oficina.\r\n</p>","ocultoPublico":false,"idOficina":128,"codigoOculto":"","canalPrivado":false,"integracionBDC":false,"codigoURL":null,"idOpcionCitacion":0,"comprobacionPreviaWeb":false,"integracionSUPRA":false},{"camposAdicionales":[],"categoria":"Identificación Electrónica","codigo360":"","codigoIntegracion":"CLAVE","entidad":null,"idTipoServicio":601,"idTramite":2290,"avisoPublico":null,"nombreTramite":"Cl@ve","oficina":"OAC Fuencarral","urlInformacion":"https://madrid.es/go/clave","familiaCita":"Cl@ve","idFamiliaCita":261,"avisoFamilia":"<p>\r\n\tPor cada cita de&nbsp;<strong>Cl@ve</strong>&nbsp;se tramitará una única gestión de: alta de<strong><span style=\"font-weight: 400;\">&nbsp;Cl@ve</span></strong>, modificación de datos de contacto, regeneración de código de activación, nivel de registro avanzado, renuncia a Cl@ve y revocación del certificado centralizado firma electrónica asociada a Cl@ve.&nbsp; \r\n</p>\r\n<p>\r\n\tEl trámite no se puede realizar por medio de representante,<strong>&nbsp;debe acudir el interesado/a.</strong> \r\n</p>\r\n<p>\r\n\tTambién puede <a href=\"https://clave.gob.es/registro/como-puedo-registrarme/registro-basico-internet-videoidentificacion-automatica\">darse de alta en clave de manera autónoma mediante videoidentificación a través de internet</a>.\r\n</p>","ocultoPublico":false,"idOficina":128,"codigoOculto":"","canalPrivado":false,"integracionBDC":false,"codigoURL":null,"idOpcionCitacion":0,"comprobacionPreviaWeb":false,"integracionSUPRA":false},{"camposAdicionales":[],"categoria":"Contribuyente","codigo360":"","codigoIntegracion":"","entidad":null,"idTipoServicio":31,"idTramite":1269,"avisoPublico":"<p>\r\n\tSi requiere ayuda por motivo de discapacidad, acuda al punto central de la oficina.<br /> \r\n</p>\r\n<p>\r\n\t<br />\r\n</p>","nombreTramite":"Tasas e impuestos","oficina":"OAC Fuencarral","urlInformacion":"https://madrid.es/go/tasas-impuestos","familiaCita":"Domiciliaciones pagos y recibos en LINEA MADRID","idFamiliaCita":302,"avisoFamilia":"<p>\r\n\tPor cada cita se realizarán hasta un máximo de 2 gestiones: duplicados de recibos, domiciliaciones y pagos con tarjeta, tanto en período voluntario como en preapremio para: IVTM, TPV, IBI, TGR, TRUA, IAE y OCU. <strong>Consulte otras vías para obtenerlos:</strong> Teléfono 010, <a href=\"https://agenciatributaria.madrid.es/portales/contribuyente/es/Gestiones-y-tramites/?vgnextfmt=default&amp;vgnextchannel=dc52e5bcc9c78710VgnVCM1000008a4a900aRCRD\">Portal del Contribuyente</a><br />\r\n</p>","ocultoPublico":false,"idOficina":128,"codigoOculto":"","canalPrivado":false,"integracionBDC":false,"codigoURL":null,"idOpcionCitacion":0,"comprobacionPreviaWeb":false,"integracionSUPRA":false},{"camposAdicionales":[],"categoria":"Urbanismo y Vivienda","codigo360":"","codigoIntegracion":"INURB","entidad":null,"idTipoServicio":24,"idTramite":1049,"avisoPublico":null,"nombreTramite":"Información urbanística","oficina":"OAC Fuencarral","urlInformacion":"","familiaCita":"Información urbanística general de las oficinas de atención a la ciudadanía","idFamiliaCita":1,"avisoFamilia":"<p>\r\n\t<strong>A primera hora de la mañana, las oficinas de Línea Madrid abren sus agendas para incluir nuevas citas.</strong><br /> \r\n</p>\r\n<p>\r\n\tConsultas generales sobre urbanismo: licencias urbanísticas vivienda y actividad, Declaración Responsable, Autorizaciones, Inspección Técnica de Edificios, Censo de locales normativa urbanística. <strong>No se informa sobre expedientes abiertos.</strong><br /> \r\n</p>\r\n<p>\r\n\tLa atención de cada cita está proyectada en un cuarto de hora. La respuesta de <strong>i</strong><strong>nformación urbanística recoge un criterio orientativo y no vinculante</strong>.<br /> \r\n</p>\r\n<p>\r\n\tSi requiere ayuda por motivo de discapacidad, acuda al punto central de la oficina.\r\n</p>","ocultoPublico":false,"idOficina":128,"codigoOculto":"","canalPrivado":false,"integracionBDC":false,"codigoURL":null,"idOpcionCitacion":0,"comprobacionPreviaWeb":false,"integracionSUPRA":false},{"camposAdicionales":[],"categoria":"Registro","codigo360":"","codigoIntegracion":"","entidad":null,"idTipoServicio":341,"idTramite":1069,"avisoPublico":null,"nombreTramite":"Registro","oficina":"OAC Fuencarral","urlInformacion":"https://madrid.es/go/oamr","familiaCita":"Registro","idFamiliaCita":241,"avisoFamilia":"<p>\r\n\tSolo se puede concertar <strong>una cita por día</strong> y presentar <strong>dos solicitudes por cita</strong>. \r\n</p>\r\n<p>\r\n\tLas personas físicas pueden presentar sus solicitudes de forma electrónica.&nbsp;<strong>Los profesionales que requieren colegiación y las personas jurídicas y sus representantes están obligados a la presentación exclusivamente electrónica.</strong> \r\n</p>\r\n<p>\r\n\tTambién puede ser atendido <strong>SIN cita</strong> en <a href=\"https://www.madrid.es/portales/munimadrid/es/Inicio/Contacto/Informacion-en-materia-de-Registro/Oficinas-de-asistencia-en-materia-de-registro-/Oficinas-de-atencion-en-materia-de-registro-de-areas-de-gobierno-municipales/?vgnextfmt=default&amp;vgnextoid=ba8395b649f05810VgnVCM2000001f4a900aRCRD&amp;vgnextchannel=61600c10c6b05810VgnVCM1000001d4a900aRCRD\">otras Oficinas municipales de Áreas y Organismos Autónomos</a>&nbsp;o&nbsp;<a href=\"https://administracion.gob.es/pagFront/atencionCiudadana/oficinas/encuentraOficina.htm\">buscar oficinas de otras Administraciones que atienden el trámite de registro</a>.\r\n</p>","ocultoPublico":false,"idOficina":128,"codigoOculto":"","canalPrivado":false,"integracionBDC":false,"codigoURL":null,"idOpcionCitacion":0,"comprobacionPreviaWeb":false,"integracionSUPRA":false},{"camposAdicionales":[],"categoria":"Padrón y censo","codigo360":"","codigoIntegracion":"EXTRJ","entidad":null,"idTipoServicio":29,"idTramite":1229,"avisoPublico":null,"nombreTramite":"Renovación extranjeros","oficina":"OAC Fuencarral","urlInformacion":"https://madrid.es/go/padron-renovacionextranjeros","familiaCita":"Renovación o confirmación en Padrón de personas extranjeras","idFamiliaCita":308,"avisoFamilia":"<p>\r\n\tPuede realizar <strong>On-Line</strong> el <a href=\"https://madrid.es/go/RenovacionConfirmacionExtranjero\">Trámite de Renovación y Confirmación para extranjeros</a>, sin necesidad de acudir presencialmente a una oficina. <br /> \r\n</p>\r\n<p>\r\n\tPara ello, deberá <strong>disponer del código CSVT</strong>, que se incluye en la notificación que haya recibido, ya sea electrónica o postal.&nbsp; \r\n</p>\r\n<p>\r\n\tPara realizar el trámite de manera presencial consulte la apertura de agendas en <a href=\"https://www.madrid.es/portales/munimadrid/es/Inicio/El-Ayuntamiento/Atencion-a-la-ciudadania/Oficinas-de-Atencion-a-la-Ciudadania/?vgnextfmt=default&amp;vgnextchannel=5b99cde2e09a4310VgnVCM1000000b205a0aRCRD\">Oficinas de Atención a la Ciudadanía.</a><br /> \r\n</p>\r\n<p>\r\n\tPor cada cita concertada se realizarán como máximo las renovaciones o confirmaciones incluidas en cada unidad conviviente en el domicilio. No olvide acudir con la carta de renovación o confirmación firmada y el documento de identidad original y en vigor. Consulte la documentación necesaria en Madrid.es para la obtención del código CSVT. <br /> \r\n</p>\r\n<p>\r\n\tSi requiere ayuda por motivo de discapacidad, acuda al punto central de la oficina.\r\n</p>","ocultoPublico":false,"idOficina":128,"codigoOculto":"","canalPrivado":false,"integracionBDC":false,"codigoURL":null,"idOpcionCitacion":0,"comprobacionPreviaWeb":true,"integracionSUPRA":false}],"urlInformacion":"https://madrid.es/go/EEIV","codIntegracion":"OAC-FUENC","codigoIntegracionExterna":null,"seleccionado":false,"idTipoServicio":24,"idPadre":0,"direccion":"Av. Monforte de Lemos, 40","visible":false,"codigoDistrito":"08","nombreDistrito":"Fuencarral- el Pardo"}
