//! Relógio do canto inferior direito (útil em tela cheia): hora local, `HH:MM`
//! (ou `h:MM AM/PM`), atualizado na virada do minuto.

use std::time::Duration;

use chrono::{Local, Timelike};

/// A hora local como texto.
pub fn agora(hour12: bool) -> String {
    formatar(Local::now().hour(), Local::now().minute(), hour12)
}

pub fn formatar(h: u32, m: u32, hour12: bool) -> String {
    if hour12 {
        let (h12, sufixo) = match h {
            0 => (12, "AM"),
            1..=11 => (h, "AM"),
            12 => (12, "PM"),
            _ => (h - 12, "PM"),
        };
        format!("{h12}:{m:02} {sufixo}")
    } else {
        format!("{h:02}:{m:02}")
    }
}

/// Quanto falta para o minuto virar (com uma folga de 50 ms para já cair no minuto novo).
pub fn ate_proximo_minuto() -> Duration {
    let t = Local::now();
    let resto_ms = 60_000u64.saturating_sub(t.second() as u64 * 1000 + t.timestamp_subsec_millis().min(999) as u64);
    Duration::from_millis(resto_ms + 50)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn formatos_de_24_e_12_horas() {
        assert_eq!(formatar(9, 5, false), "09:05");
        assert_eq!(formatar(23, 59, false), "23:59");
        assert_eq!(formatar(0, 0, true), "12:00 AM");
        assert_eq!(formatar(12, 30, true), "12:30 PM");
        assert_eq!(formatar(15, 7, true), "3:07 PM");
        assert_eq!(formatar(11, 59, true), "11:59 AM");
    }

    #[test]
    fn espera_nunca_passa_de_um_minuto() {
        let d = ate_proximo_minuto();
        assert!(d > Duration::from_millis(40) && d <= Duration::from_millis(60_050), "{d:?}");
    }
}
