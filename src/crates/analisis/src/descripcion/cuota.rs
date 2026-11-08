//! Gemini's quota: each model's requests counted by the day, and when the
//! day turns. The free plan limits requests per model and per day (twenty
//! a day for the newest Flash, say), and the day turns at midnight in
//! California - nine or ten in the morning in Spain. Failed attempts on an
//! overloaded model count too, so a call is retried little and moves on to
//! the next model. The engine's `motor/cuota.py`.
//!
//! Kept in `cuota_google.json` in the cache folder, shared by every job on
//! this computer and by the engine:
//! `{"dia": "2026-10-22", "usadas": {"gemini-flash-latest": 7},
//!   "limites": {"gemini-flash-latest": 20},
//!   "agotados": {"gemini-flash-latest": "2026-10-23T09:00:00+02:00"}}`

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Datelike, Duration as Lapso, FixedOffset, Local, NaiveDate, TimeZone, Utc, Weekday};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// The file's name in the cache folder.
pub const ARCHIVO: &str = "cuota_google.json";

/// Only one thread at a time reads and writes the file.
static CERROJO: Mutex<()> = Mutex::new(());

/// The quota file in a cache folder.
#[derive(Clone, Debug)]
pub struct Cuota {
    ruta: PathBuf,
}

/// What the file holds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Datos {
    dia: String,
    usadas: BTreeMap<String, u32>,
    limites: BTreeMap<String, u32>,
    agotados: BTreeMap<String, String>,
}

impl Cuota {
    /// The one in the cache folder `carpeta`.
    pub fn en(carpeta: PathBuf) -> Self {
        Self {
            ruta: carpeta.join(ARCHIVO),
        }
    }

    /// Counts a request to `modelo`.
    pub fn registrar(&self, modelo: &str) {
        self.cambiar(|datos| *datos.usadas.entry(modelo.to_owned()).or_default() += 1);
    }

    /// Notes that `modelo` has no requests left today, and the limit
    /// Google's `error` names ("limit: 20"). Returns when it can be asked
    /// again: what the error says to wait, or the next renewal.
    pub fn marcar_agotado(&self, modelo: &str, error: &str) -> DateTime<FixedOffset> {
        let reanudar = match espera_pedida(error) {
            Some(espera) => (Local::now() + espera + Lapso::seconds(60)).fixed_offset(),
            None => proxima_renovacion(Utc::now()),
        };
        let limite = Regex::new(r"limit: (\d+)")
            .ok()
            .and_then(|re| re.captures(error)?.get(1)?.as_str().parse().ok());
        self.cambiar(|datos| {
            if let Some(limite) = limite {
                datos.limites.insert(modelo.to_owned(), limite);
            }
            datos
                .agotados
                .insert(modelo.to_owned(), reanudar.to_rfc3339_opts(chrono::SecondsFormat::Secs, false));
        });
        reanudar
    }

    /// Whether `modelo` is out of requests for now.
    pub fn agotado(&self, modelo: &str) -> bool {
        self.hasta(modelo).is_some_and(|hasta| hasta > Utc::now())
    }

    /// When `modelo` can be asked again, if it is out.
    pub fn hasta(&self, modelo: &str) -> Option<DateTime<FixedOffset>> {
        let datos = self.leer();
        DateTime::parse_from_rfc3339(datos.agotados.get(modelo)?).ok()
    }

    /// The requests `modelo` has left today, by its known limit or
    /// `limite_config`; None when no limit is known - a paid key.
    pub fn restantes(&self, modelo: &str, limite_config: Option<u32>) -> Option<u32> {
        if self.agotado(modelo) {
            return Some(0);
        }
        let datos = self.leer();
        let limite = datos.limites.get(modelo).copied().or(limite_config)?;
        Some(limite.saturating_sub(datos.usadas.get(modelo).copied().unwrap_or(0)))
    }

    fn leer(&self) -> Datos {
        let _cerrado = CERROJO.lock();
        self.leer_sin_cerrojo(Utc::now())
    }

    /// The file as of `ahora`: a new day starts from nothing used, the
    /// limits learnt kept.
    fn leer_sin_cerrojo(&self, ahora: DateTime<Utc>) -> Datos {
        let datos: Datos = std::fs::read_to_string(&self.ruta)
            .ok()
            .and_then(|texto| serde_json::from_str(&texto).ok())
            .unwrap_or_default();
        let hoy = dia(ahora);
        if datos.dia == hoy {
            datos
        } else {
            Datos {
                dia: hoy,
                limites: datos.limites,
                ..Datos::default()
            }
        }
    }

    fn cambiar(&self, cambio: impl FnOnce(&mut Datos)) {
        let _cerrado = CERROJO.lock();
        let mut datos = self.leer_sin_cerrojo(Utc::now());
        cambio(&mut datos);
        let escrito = self
            .ruta
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                let texto = serde_json::to_string_pretty(&datos).map_err(std::io::Error::other)?;
                std::fs::write(&self.ruta, texto)
            });
        if let Err(error) = escrito {
            log::warn!("{}: quota not kept: {error}", self.ruta.display());
        }
    }
}

/// What Google says to wait in `error`: "retry in 1h2m3.5s".
fn espera_pedida(error: &str) -> Option<Lapso> {
    let re = Regex::new(r"retry in (?:(\d+)h)?(?:(\d+)m)?(?:([\d.]+)s)?").ok()?;
    let partes = re.captures(error)?;
    let numero = |n: usize| partes.get(n).and_then(|m| m.as_str().parse::<f64>().ok());
    let (horas, minutos, segundos) = (numero(1), numero(2), numero(3));
    if horas.is_none() && minutos.is_none() && segundos.is_none() {
        return None;
    }
    let total = horas.unwrap_or(0.0) * 3600.0 + minutos.unwrap_or(0.0) * 60.0 + segundos.unwrap_or(0.0);
    Some(Lapso::milliseconds((total * 1000.0) as i64))
}

/// The seconds a per-minute limit asks to wait: Google's `retryDelay`, or
/// "retry in 12s"; 30 when it does not say, 90 at most.
pub fn espera_por_minuto(error: &str) -> Duration {
    let patrones = [r#"retry(?:Delay)?['"]?\s*[:=]\s*['"]?(\d+(?:\.\d+)?)s"#, r"retry in ([\d.]+)s"];
    let segundos = patrones.iter().find_map(|patron| {
        Regex::new(patron)
            .ok()?
            .captures(error)?
            .get(1)?
            .as_str()
            .parse::<f64>()
            .ok()
    });
    Duration::from_secs_f64(segundos.map_or(30.0, |s| (s + 2.0).min(90.0)))
}

/// California's offset from UTC at `ahora`: summer time from the second
/// Sunday of March to the first of November, both at 2 a.m. local.
fn pacifico(ahora: DateTime<Utc>) -> FixedOffset {
    let ano = ahora.year();
    let domingo = |mes: u32, n: u8| NaiveDate::from_weekday_of_month_opt(ano, mes, Weekday::Sun, n);
    let (Some(marzo), Some(noviembre)) = (domingo(3, 2), domingo(11, 1)) else {
        return FixedOffset::west_opt(8 * 3600).expect("a valid offset");
    };
    let empieza = Utc.from_utc_datetime(&marzo.and_hms_opt(10, 0, 0).unwrap_or_default());
    let acaba = Utc.from_utc_datetime(&noviembre.and_hms_opt(9, 0, 0).unwrap_or_default());
    let horas = if (empieza..acaba).contains(&ahora) { 7 } else { 8 };
    FixedOffset::west_opt(horas * 3600).expect("a valid offset")
}

/// The quota's day at `ahora`: California's date.
fn dia(ahora: DateTime<Utc>) -> String {
    ahora.with_timezone(&pacifico(ahora)).date_naive().to_string()
}

/// Five past the next midnight in California, in this computer's time.
pub fn proxima_renovacion(ahora: DateTime<Utc>) -> DateTime<FixedOffset> {
    let manana = ahora.with_timezone(&pacifico(ahora)).date_naive() + Lapso::days(1);
    let medianoche = manana.and_hms_opt(0, 5, 0).unwrap_or_default();
    let alla = pacifico(Utc.from_utc_datetime(&(medianoche + Lapso::hours(8))));
    let renovacion = alla
        .from_local_datetime(&medianoche)
        .single()
        .map_or(ahora, |r| r.with_timezone(&Utc));
    renovacion.with_timezone(&Local).fixed_offset()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(texto: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(texto).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn the_day_turns_at_midnight_in_california() {
        // Summer: UTC-7, so 06:59 UTC is still the day before.
        assert_eq!(dia(utc("2026-10-23T06:59:00Z")), "2026-10-22");
        assert_eq!(dia(utc("2026-10-23T07:01:00Z")), "2026-10-23");
        // Winter: UTC-8.
        assert_eq!(dia(utc("2026-12-01T07:30:00Z")), "2026-11-30");
        assert_eq!(dia(utc("2026-12-01T08:30:00Z")), "2026-12-01");
        assert_eq!(
            proxima_renovacion(utc("2026-10-22T12:00:00Z")).with_timezone(&Utc),
            utc("2026-10-23T07:05:00Z")
        );
        assert_eq!(
            proxima_renovacion(utc("2026-12-01T12:00:00Z")).with_timezone(&Utc),
            utc("2026-12-02T08:05:00Z")
        );
    }

    #[test]
    fn google_s_waits_are_read() {
        assert_eq!(espera_pedida("Please retry in 1h2m3s."), Some(Lapso::seconds(3723)));
        assert_eq!(espera_pedida("retry in 12.5s"), Some(Lapso::milliseconds(12_500)));
        assert_eq!(espera_pedida("no"), None);
        assert_eq!(espera_por_minuto(r#""retryDelay": "17s""#), Duration::from_secs(19));
        assert_eq!(espera_por_minuto("retry in 200s"), Duration::from_secs(90));
        assert_eq!(espera_por_minuto("nada"), Duration::from_secs(30));
    }

    #[test]
    fn requests_are_counted_against_the_limit_google_named() {
        let carpeta = std::env::temp_dir().join(format!("analisis-cuota-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        let cuota = Cuota::en(carpeta);
        assert_eq!(cuota.restantes("flash", None), None);
        assert_eq!(cuota.restantes("flash", Some(20)), Some(20));
        cuota.registrar("flash");
        cuota.registrar("flash");
        assert_eq!(cuota.restantes("flash", Some(20)), Some(18));

        cuota.marcar_agotado("flash", "Quota exceeded ... limit: 25 ... per day");

        assert!(cuota.agotado("flash"));
        assert_eq!(cuota.restantes("flash", None), Some(0));
        assert!(!cuota.agotado("pro"));
        let datos = cuota.leer();
        assert_eq!(datos.limites["flash"], 25);
    }
}
